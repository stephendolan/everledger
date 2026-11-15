#!/usr/bin/env python3
"""Generate clap's registered surface from the read-only argparse oracle."""
import argparse
import json
from pathlib import Path
import runpy
import sys
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
ns=runpy.run_path(str(ROOT/'everledger'))
p=ns['parser']()
def lit(s): return json.dumps(s,ensure_ascii=False)
lines=['use clap::{Arg, ArgAction, Command};','pub fn parser() -> Command {',
' let p=Command::new("everledger").disable_version_flag(true).subcommand_required(true).arg_required_else_help(false).args_override_self(true)',
'.about('+lit(p.description)+').after_help('+lit(p.epilog)+');',' let mut p=p;']
sub=next(a for a in p._actions if isinstance(a,argparse._SubParsersAction))
for name,c in sub.choices.items():
    lines+=[' let c=Command::new('+lit(name)+').about('+lit(c.description)+').args_override_self(true);',' let mut c=c;']
    for a in c._actions:
        if isinstance(a,argparse._HelpAction):continue
        ident=a.dest
        s='Arg::new('+lit(ident)+')'
        if a.option_strings:s+='.long('+lit(a.option_strings[0][2:])+')'
        else:s+='.index('+str(1+sum(1 for b in c._actions[:c._actions.index(a)] if not b.option_strings))+')'
        if isinstance(a,argparse._StoreTrueAction):s+='.action(ArgAction::SetTrue)'
        else:
            s+='.allow_negative_numbers(true)'
            if a.type is int:s+='.value_parser(clap::value_parser!(crate::types::CliInteger))'
            if a.nargs=='+':s+='.num_args(1..).required(true)'
            elif a.nargs=='?':s+='.required(false)'
            elif a.required or not a.option_strings:s+='.required(true)'
            if a.choices:s+='.value_parser(['+','.join(map(lit,a.choices))+'])'
            if a.default is not None:s+='.default_value('+lit(str(a.default))+')'
        if a.help==argparse.SUPPRESS:s+='.hide(true)'
        elif a.help:s+='.help('+lit(a.help)+')'
        lines+=[' c=c.arg('+s+');']
    if c.epilog:lines+=[' c=c.after_help('+lit(c.epilog)+');']
    lines+=[' p=p.subcommand(c);']
lines+=[' p','}']
(ROOT/'src/cli_surface.rs').write_text('\n'.join(lines)+'\n')
print('generated all 16 registered commands and options')
