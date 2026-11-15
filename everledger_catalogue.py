from everledger_core import human, money, roster, character_path, recipes as all_recipes

ANCHORS = [("tailoring", "White Swashbuckler's Shirt", 125, "Trainer"),
           ("tailoring", "Bolt of Silk Cloth", 100, "Trainer"),
           ("tailoring", "Bolt of Woolen Cloth", 55, "Trainer"),
           ("tailoring", "Red Woolen Boots", 75, "Pattern · Drop"),
           ("tailoring", "Dark Silk Shirt", 120, "Pattern · Vendor"),
           ("tailoring", "Robes of Arcana", 125, "Pattern · Drop"),
           ("leatherworking", "Cloudy Gustwoven Boots", 55, None)]


def color_at(bands, skill):
    if skill < bands["learn"]: return "not learnable"
    if bands["yellow"] is None: return "orange"
    if skill < bands["yellow"]: return "orange"
    if skill < bands["green"]: return "yellow"
    if skill < bands["grey"]: return "green"
    return "grey"


def find(cat, query):
    q = query.lower()
    hits = [(p, r) for p, r in all_recipes(cat) if q in r["name"].lower()]
    exact = [(p, r) for p, r in hits if r["name"].lower() == q]
    return exact or hits


def fmt_recipe(prof, r, skill=None):
    b = r["bands"]
    bands = f"learn {b['learn']}" + (f", yellow {b['yellow']}, green {b['green']}, grey {b['grey']}" if b["yellow"] is not None else " (stays orange)")
    col = f" | at skill {skill}: {color_at(b, skill)}" if skill is not None else ""
    reags = ", ".join(f"{x['qty']}x {x['name']}" for x in r["reagents"]) or "none"
    res = r["result"]
    res_s = f"{res['name']}" + (f" (item {res['item_id']})" if res["item_id"] else "") + (f" x{res['qty']}" if res["qty"] > 1 else "")
    return (f"{r['name']} [{prof}] spell {r['spell_id']}\n"
            f"  bands: {bands}{col}\n  makes: {res_s}\n  reagents: {reags}\n  source: {r['source_type'] or 'unmarked in index (see recipe page)'}")


def spell_index(cat):
    return {r['spell_id']: (prof, r['name']) for prof, r in all_recipes(cat)}


def char_known_ids(ch, prof_name):
    for p in ch.get("professions", []):
        if p["name"].lower() == prof_name.lower():
            rec = p.get("recipes") or {}
            if rec.get("available") and rec.get("known") is not None:
                return {k["spell_id"] for k in rec["known"]}
            return None
    return None


def cmd_char(cat, args, data=None):
    skill_override = None
    if "--skill" in args:
        i = args.index("--skill"); skill_override = int(args[i + 1]); args = args[:i] + args[i + 2:]
    name = args[0]
    data = data if data is not None else roster(character_path(name))
    ch = data.get("character", data)
    print(f"{human(ch.get('name'))} | level {human(ch.get('level'))} {human(ch.get('race'))} {human(ch.get('class'))} | {human(ch.get('faction'))} {human(ch.get('realm'))} | gold {money(ch.get('gold_copper'))} | hearth {human((ch.get('hearth') or {}).get('location'))}")
    print(f"  (roster snapshot generated {(data.get('generated_at') or '?')[:16]}Z; professions updated {(ch.get('professions_updated_at') or '?')[:16]}Z)")
    want = args[1].lower() if len(args) > 1 else None
    for p in ch.get("professions", []):
        if p.get("skill_available"):
            print(f"  {p['name']}: {human(p['skill'])}/{human(p['max_skill'])}")
        else:
            print(f"  {p['name']}: skill unavailable from roster ({human(p.get('reason'))}); open the profession window in game, or pass --skill")
        rec = p.get("recipes") or {}
        if rec.get("available") and rec.get("known") is not None:
            print(f"    known recipes: {len(rec['known'])} (roster, DataStore_Crafts)")
        else:
            print(f"    known recipes: unavailable from roster (never read as 'knows nothing')")
        if want and p["name"].lower() == want:
            sk = skill_override if skill_override is not None else p.get("skill")
            if sk is None:
                print("    no skill to analyse; pass --skill N")
                continue
            blk = cat["professions"].get(want)
            if not blk: print(f"    no catalogue profession '{want}'"); continue
            k_ids = char_known_ids(ch, p["name"])
            k_avail = k_ids is not None
            tag = (lambda r: "known " if r["spell_id"] in k_ids else "TRAIN ") if k_avail else (lambda r: "")
            ups = [r for r in blk["recipes"] if r["bands"]["learn"] <= sk and (r["bands"]["grey"] is None or r["bands"]["grey"] > sk)]
            crank = {"orange": 0, "yellow": 1, "green": 2}
            ups.sort(key=lambda r: (crank.get(color_at(r["bands"], sk), 3), -r["bands"]["learn"]))
            print(f"    skill-up options at {sk}:" + (" (TRAIN = not known; training/pattern cost applies)" if k_avail else ""))
            for r in ups[:8]:
                print(f"      {tag(r)}{r['name']:<34} {color_at(r['bands'], sk):<6} learn {r['bands']['learn']:<4} grey {human(r['bands']['grey'])}  {r['source_type'] or '?'}")
            nxt = sorted([r for r in blk["recipes"] if sk < r["bands"]["learn"] <= sk + 30], key=lambda r: r["bands"]["learn"])
            print(f"    next unlocks (to {sk + 30}):")
            for r in nxt[:8]:
                print(f"      {tag(r)}{r['name']:<34} learn {r['bands']['learn']}  {r['source_type'] or '?'}")
            if k_avail:
                idx = spell_index(cat)
                names = sorted(idx[s][1] for s in k_ids if s in idx and idx[s][0] == want)
                extra = len(k_ids) - sum(1 for s in k_ids if s in idx and idx[s][0] == want)
                print(f"    all known {p['name']} recipes ({len(k_ids)}):")
                for i in range(0, len(names), 4):
                    print("      " + ", ".join(names[i:i + 4]))
                if extra: print(f"      (+{extra} known spell ids not in the catalogue for this profession)")
    return 0


def cmd_verify(cat, _args):
    ok = True
    for prof, blk in cat["professions"].items():
        good = len(blk["recipes"]) == blk["stated_count"]
        ok = ok and good
        print(f"{'PASS' if good else 'FAIL'} {prof}: {len(blk['recipes'])}/{blk['stated_count']} recipes")
    for prof, name, learn, src in ANCHORS:
        r = next((x for x in cat["professions"].get(prof, {}).get("recipes", []) if x["name"] == name), None)
        good = bool(r) and r["bands"]["learn"] == learn and (src is None or r["source_type"] == src)
        ok = ok and good
        print(f"{'PASS' if good else 'FAIL'} anchor {name}: learn {r['bands']['learn'] if r else '?'}, source {human(r['source_type']) if r else '?'}")
    print("catalogue built:", cat["generated_at"], "| source:", cat["source"])
    return 0 if ok else 1
