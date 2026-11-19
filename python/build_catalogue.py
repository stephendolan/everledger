#!/usr/bin/env python3
"""Build the WoW Forever recipe catalogue from wowforever.ninja profession index pages.
Source of truth: Forever client data as published by wowforever.ninja (server-rendered tables).
Every recipe row carries: spell id, learn/yellow/green/grey bands, result item id,
reagents with item ids, and the source type. AHledger is never used for these fields."""
import html, json, os, re, sys, datetime, pathlib

DATA_DIR = pathlib.Path(os.environ.get("EVERLEDGER_DATA_DIR", pathlib.Path.home() / ".everledger"))
RAW = DATA_DIR / "raw"
OUT = DATA_DIR / "catalogue.json"
PROFS = ["alchemy","blacksmithing","enchanting","engineering","leatherworking",
         "tailoring","cooking","first-aid","fishing","mining","herbalism","skinning"]

ROW = re.compile(r'<tr role="row"(.*?)</tr>', re.S)
BANDS = re.compile(r'<span class="o">(\d+)</span>(?:\s*<span class="y">(\d+)</span>)?(?:\s*<span class="g">(\d+)</span>)?(?:\s*<span class="x">(\d+)</span>)?')
RLINK = re.compile(r'href="/professions/[^/"]+/(\d+)-[^"]*/"[^>]*>(.*?)</a>', re.S)
ILINK = re.compile(r'href="/items/(\d+)-[^"]*/"[^>]*>(?:<img[^>]*>)?\s*(.*?)</a>(\s*×\s*(\d+))?', re.S)
CELL = re.compile(r'<td[^>]*role="cell"[^>]*>(.*?)</td>', re.S)
COUNT = re.compile(r'Recipes \((\d+)\)')

def text(s):
    return html.unescape(re.sub(r'<[^>]+>', '', s)).strip()

catalogue = {"generated_at": datetime.datetime.now(datetime.UTC).strftime("%Y-%m-%dT%H:%M:%SZ"),
             "source": "wowforever.ninja profession index pages (Forever client data)",
             "professions": {}}
report = []
for prof in PROFS:
    raw = (RAW / f"{prof}.html").read_text(encoding="utf-8", errors="replace")
    stated = int(COUNT.search(raw).group(1))
    recipes, anomalies = [], 0
    for m in ROW.finditer(raw):
        row = m.group(0)
        cells = CELL.findall(row)
        b = BANDS.search(row); r = RLINK.search(row)
        if not r:
            continue  # not a recipe row (trainer tables etc. share the markup)
        if not b or len(cells) < 4:
            anomalies += 1; continue
        items = ILINK.findall(cells[2])
        if items:
            rid, rname, _x, rqty = items[0]
        else:  # result with no item link (enchants and similar): keep the text name
            rid, rname, rqty = None, text(cells[2]), "" 
        reagents = []
        for iid, iname, _x2, iqty in ILINK.findall(cells[3]):
            reagents.append({"item_id": int(iid), "name": text(iname), "qty": int(iqty) if iqty else 1})
        recipes.append({
            "spell_id": int(r.group(1)), "name": text(r.group(2)),
            "bands": {"learn": int(b.group(1)), "yellow": (int(b.group(2)) if b.group(2) else None), "green": (int(b.group(3)) if b.group(3) else None), "grey": (int(b.group(4)) if b.group(4) else None)},
            "result": {"item_id": (int(rid) if rid is not None else None), "name": text(rname), "qty": int(rqty) if rqty else 1},
            "reagents": reagents, "source_type": (text(cells[4]) if len(cells) > 4 else None)})
    catalogue["professions"][prof] = {"stated_count": stated, "recipes": recipes}
    flag = "OK" if len(recipes) == stated and anomalies == 0 else f"CHECK anomalies={anomalies}"
    report.append(f"{prof}: stated {stated}, parsed {len(recipes)} {flag}")

OUT.write_text(json.dumps(catalogue, indent=1, ensure_ascii=False))
print("\n".join(report))
print("catalogue written:", OUT, f"({OUT.stat().st_size/1024:.0f} KiB)")

# anchor checks: recipes whose AHledger metadata (learn level, source type) is known
# to be wrong; the catalogue values asserted below are the verified ones
def find(prof, name):
    for r in catalogue["professions"][prof]["recipes"]:
        if r["name"] == name: return r
    return None
anchors = [("tailoring","White Swashbuckler's Shirt",125,"Trainer"),
           ("tailoring","Bolt of Silk Cloth",100,"Trainer"),
           ("tailoring","Bolt of Woolen Cloth",55,None),
           ("tailoring","Red Woolen Boots",75,"Pattern · Drop"),
           ("tailoring","Dark Silk Shirt",120,"Pattern · Vendor"),
           ("tailoring","Robes of Arcana",125,"Pattern · Drop"),
           ("leatherworking","Cloudy Gustwoven Boots",55,None)]
print("\n== anchors ==")
ok = True
for prof, name, learn, src in anchors:
    r = find(prof, name)
    if not r: print(f"MISSING {prof}/{name}"); ok = False; continue
    good = r["bands"]["learn"] == learn and (src is None or r["source_type"] == src)
    ok = ok and good
    print(f"{'PASS' if good else 'FAIL'} {name}: learn {r['bands']['learn']} (want {learn}), source {r['source_type']}" + (f" (want {src})" if src else ""))
print("anchors:", "ALL PASS" if ok else "FAILURES PRESENT")
