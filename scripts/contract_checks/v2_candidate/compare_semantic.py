"""Compare candidate boundaries to an exact disposable approved-core snapshot.

Run manually with the existing offline Rust toolchain and --source. The sibling
checkout is read-only; only a git archive under work/ receives temporary tests.
"""
import argparse
import io
import json
import os
import re
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
from boundaries import cases
from profile import SEMANTIC_COMMIT
from reconstruct import ROOT


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--source',required=True,type=Path)
    parser.add_argument('--capture-originals',action='store_true',help='Write authoritative synthetic Evaluation bytes under work/ only')
    args=parser.parse_args()
    env=dict(os.environ,DEVELOPER_DIR='/Library/Developer/CommandLineTools')
    sha=subprocess.check_output(['git','rev-parse','HEAD'],cwd=args.source,env=env,text=True).strip()
    assert sha==SEMANTIC_COMMIT, 'WRONG_SEMANTIC_COMMIT'
    # Produce current, validated attack diagnostics for the Rust parity tests.
    # The audit is read-only for candidate sources and never authors goldens.
    if not args.capture_originals:
        subprocess.run([sys.executable,str(Path(__file__).with_name('audit.py'))],cwd=ROOT,env=env,check=True)
    archive=subprocess.check_output(['git','archive',SEMANTIC_COMMIT],cwd=args.source,env=env)
    work=ROOT/'work/semantic-comparison';work.mkdir(parents=True,exist_ok=True)
    snapshot=Path(tempfile.mkdtemp(prefix='approved-',dir=work))
    with tarfile.open(fileobj=io.BytesIO(archive)) as source:
        source.extractall(snapshot,filter='data')
    # Derive the complete model representation in the disposable snapshot only.
    # No production source in either assigned/sibling worktree is edited.
    model=snapshot/'crates/ledgerlab-core/src/policy/chaining/model.rs'
    code=model.read_text()
    def derives(match):
        kind,name=match.groups()
        if name in ('Input','Explanation'):return match.group(0)
        traits='serde::Serialize' if name=='Explanation' else 'serde::Serialize, serde::Deserialize'
        return '#[derive('+traits+')]\n#[serde(deny_unknown_fields)]\n'+match.group(0)
    code=re.sub(r'pub (struct|enum) (\w+)',derives,code)
    code=re.sub(r'(?m)^(    pub(?:\(super\))? \w+: Option<[^\n]+)',r'    #[serde(default, skip_serializing_if = "Option::is_none")]\n\1',code)
    start=code.index('pub struct Explanation {');end=code.index('\n}',start)
    code=code[:start]+code[start:end].replace('    #[serde(default, skip_serializing_if = "Option::is_none")]\n','')+code[end:]
    code += r'''
impl serde::Serialize for Explanation {
    fn serialize<S:serde::Serializer>(&self,s:S)->std::result::Result<S::Ok,S::Error> {
        let mut v=serde_json::json!({"binding_id":self.binding_id,"code":self.code,"inputs":self.inputs,"action_ids":self.action_ids});
        let map=v.as_object_mut().unwrap();
        if let Some(x)=&self.rule_id {map.insert("rule_id".into(),serde_json::json!(x));}
        if let Some(x)=&self.basis {map.insert("basis".into(),serde_json::json!(x));}
        if let Some(x)=&self.unrounded {map.insert("unrounded".into(),serde_json::json!(x));}
        if let Some(x)=self.rounded {map.insert("rounded".into(),serde_json::json!(x.to_string()));}
        serde::Serialize::serialize(&v,s)
    }
}
impl<'de> serde::Deserialize<'de> for Explanation {
    fn deserialize<D:serde::Deserializer<'de>>(d:D)->std::result::Result<Self,D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire { binding_id:String, rule_id:Option<String>, code:String,
            basis:Option<ExactRatio>, unrounded:Option<ExactRatio>, rounded:Option<String>,
            inputs:Vec<String>, action_ids:Vec<String> }
        let w=Wire::deserialize(d)?;
        Ok(Self { binding_id:w.binding_id,rule_id:w.rule_id,
            code:Box::leak(w.code.into_boxed_str()),basis:w.basis,unrounded:w.unrounded,
            rounded:w.rounded.map(|s|crate::money::parse_atoms(&s).map_err(serde::de::Error::custom)).transpose()?,inputs:w.inputs,action_ids:w.action_ids })
    }
}
'''
    model.write_text(code)
    event=snapshot/'crates/ledgerlab-core/src/domain/event.rs'
    event.write_text(event.read_text()+Path(__file__).with_name('original_codec.rs').read_text())
    scalar_file=work/'scalar-cases.json';scalar_file.write_text(json.dumps(cases(),ensure_ascii=False))
    core=snapshot/'crates/ledgerlab-core/src/domain/mod.rs'
    core.write_text(core.read_text()+r'''
#[cfg(test)]
mod canonical_candidate_boundaries {
    #[test]
    fn approved_scalar_comparison() {
        use crate::money::{Decimal, ExactRatio, parse_atoms};
        let cases:serde_json::Value=serde_json::from_slice(&std::fs::read(std::env::var("LEDGERLAB_SCALAR_CASES").unwrap()).unwrap()).unwrap();
        for c in cases.as_array().unwrap() {
            let value=c["value"].as_str().unwrap_or("");
            let actual=match c["kind"].as_str().unwrap() {
                "text" => super::text(value,128).is_ok(),
                "source" => super::validate_source(value).is_ok(),
                "slug" => super::slug(value).is_ok(),
                "decimal" => Decimal::parse(value).is_ok(),
                "decimal-percent" => Decimal::parse(value).and_then(|d|d.percent()).is_ok(),
                "positive-decimal" => Decimal::parse(value).is_ok_and(|d| !d.is_zero()),
                "uint" => super::Revision::parse(value).is_ok(),
                "atoms" => parse_atoms(value).is_ok(),
                "nonnegative-atoms" => parse_atoms(value).is_ok_and(|n|n>=0),
                "time" => super::Timestamp::parse(value).is_ok(),
                "ratio" => ExactRatio::from_canonical(c["value"]["numerator"].as_str().unwrap(),c["value"]["denominator"].as_str().unwrap()).is_ok(),
                _ => super::prefixed(value,c["rust_prefix"].as_str().unwrap()).is_ok(),
            };
            assert_eq!(actual,c["accepted"].as_bool().unwrap(),"{c}");
        }
        println!("Approved scalar comparisons: {}",cases.as_array().unwrap().len());
        if std::env::var("LEDGERLAB_CAPTURE_ORIGINALS").is_err(){
            let root=std::env::var("LEDGERLAB_CANDIDATE_ROOT").unwrap();
            let expected:serde_json::Value=serde_json::from_slice(&std::fs::read(std::path::Path::new(&root).join("work/validation/v2-unicode-parity.json")).unwrap()).unwrap();
            let mut text=vec![];let mut source=vec![];let mut count=0;
            for cp in 0..=0x10ffff {
                let Some(character)=char::from_u32(cp) else {continue};count+=1;
                if super::text(&format!("a{character}"),128).is_err(){text.push(cp);}
                if super::validate_source(&format!("urn:synthetic:{character}outcome")).is_err(){source.push(cp);}
            }
            assert_eq!(serde_json::json!(count),expected["scalar_values"]);
            assert_eq!(serde_json::json!(text),expected["rejected"]["text"]);
            assert_eq!(serde_json::json!(source),expected["rejected"]["source"]);
            println!("Approved exhaustive Unicode parity: {count} scalar values, {} text/source checks.",count*2);
        }
    }
}
''')
    test=snapshot/'crates/ledgerlab-testkit/tests/phase2_outcomes.rs'
    test.write_text(test.read_text()+Path(__file__).with_name('semantic_comparison.rs').read_text())
    env.update(LEDGERLAB_SCALAR_CASES=str(scalar_file),LEDGERLAB_CANDIDATE_ROOT=str(ROOT),CARGO_TARGET_DIR=str(work/'target'))
    if args.capture_originals:
        capture=work/'original-evaluations';capture.mkdir(exist_ok=True);env['LEDGERLAB_CAPTURE_ORIGINALS']=str(capture)
    subprocess.run(['cargo','test','-p','ledgerlab-core','--locked','--offline','approved_scalar_comparison','--','--nocapture'],cwd=snapshot,env=env,check=True)
    subprocess.run(['cargo','test','-p','ledgerlab-testkit','--test','phase2_outcomes','--locked','--offline','--','--nocapture'],cwd=snapshot,env=env,check=True)
    print(json.dumps(dict(status='passed',semantic_commit=sha,scalar_cases=len(cases()),snapshot=str(snapshot))))


if __name__=='__main__':main()
