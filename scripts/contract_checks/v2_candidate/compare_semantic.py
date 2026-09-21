"""Compare candidate boundaries to an exact disposable approved-core snapshot.

Run manually with the existing offline Rust toolchain and --source. The sibling
checkout is read-only; only a git archive under work/ receives temporary tests.
"""
import argparse
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
from boundaries import cases
from profile import SEMANTIC_COMMIT
from reconstruct import ROOT


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--source',required=True,type=Path)
    args=parser.parse_args()
    env=dict(os.environ,DEVELOPER_DIR='/Library/Developer/CommandLineTools')
    sha=subprocess.check_output(['git','rev-parse','HEAD'],cwd=args.source,env=env,text=True).strip()
    assert sha==SEMANTIC_COMMIT, 'WRONG_SEMANTIC_COMMIT'
    archive=subprocess.check_output(['git','archive',SEMANTIC_COMMIT],cwd=args.source,env=env)
    work=ROOT/'work/semantic-comparison';work.mkdir(parents=True,exist_ok=True)
    snapshot=Path(tempfile.mkdtemp(prefix='approved-',dir=work))
    with tarfile.open(fileobj=io.BytesIO(archive)) as source:
        source.extractall(snapshot,filter='data')
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
                "time" => super::Timestamp::parse(value).is_ok(),
                "ratio" => ExactRatio::from_canonical(c["value"]["numerator"].as_str().unwrap(),c["value"]["denominator"].as_str().unwrap()).is_ok(),
                _ => panic!("unknown scalar case"),
            };
            assert_eq!(actual,c["accepted"].as_bool().unwrap(),"{c}");
        }
        println!("Approved scalar comparisons: {}",cases.as_array().unwrap().len());
    }
}
''')
    test=snapshot/'crates/ledgerlab-testkit/tests/phase2_outcomes.rs'
    test.write_text(test.read_text()+Path(__file__).with_name('semantic_comparison.rs').read_text())
    env.update(LEDGERLAB_SCALAR_CASES=str(scalar_file),LEDGERLAB_CANDIDATE_ROOT=str(ROOT),CARGO_TARGET_DIR=str(work/'target'))
    subprocess.run(['cargo','test','-p','ledgerlab-core','--locked','--offline','approved_scalar_comparison','--','--nocapture'],cwd=snapshot,env=env,check=True)
    subprocess.run(['cargo','test','-p','ledgerlab-testkit','--test','phase2_outcomes','--locked','--offline','--','--nocapture'],cwd=snapshot,env=env,check=True)
    print(json.dumps(dict(status='passed',semantic_commit=sha,scalar_cases=len(cases()),snapshot=str(snapshot))))


if __name__=='__main__':main()
