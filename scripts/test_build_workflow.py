"""Adversarial source capture checks; no publication/economic oracle."""
import importlib.util, os, subprocess, tempfile, unittest
from pathlib import Path
spec=importlib.util.spec_from_file_location('capture',Path(__file__).with_name('build_workflow.py'))
capture=importlib.util.module_from_spec(spec);spec.loader.exec_module(capture)
class CaptureTests(unittest.TestCase):
    def test_deleted_tracked_input_cannot_claim_clean_commit(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            (root/'crates/example/src').mkdir(parents=True)
            (root/'crates/example/src/lib.rs').write_bytes(b'pub fn selected() {}\n')
            subprocess.run(['git','init','-q',directory],check=True)
            subprocess.run(['git','-C',directory,'add','.'],check=True)
            subprocess.run(['git','-C',directory,'-c','user.name=Synthetic','-c','user.email=synthetic@example.invalid','commit','-qm','synthetic source'],check=True)
            base=capture.command(['git','rev-parse','HEAD'],root)
            files={'crates/example/src/lib.rs':(root/'crates/example/src/lib.rs').read_bytes()}
            self.assertTrue(capture.clean_source(root,files,base))
            (root/'crates/example/src/lib.rs').unlink()
            self.assertFalse(capture.clean_source(root,{},base))
    def test_symlink_source_input_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);(root/'crates/example').mkdir(parents=True);(root/'scripts').mkdir()
            for path in ['Cargo.toml','Cargo.lock','scripts/build_workflow.py']:(root/path).write_bytes(b'')
            (root/'crates/example/lib.rs').symlink_to(root/'Cargo.lock')
            with self.assertRaises(ValueError):capture.selected(root)
    def test_effective_compiler_override_refuses_before_capture(self):
        import sys
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as directory:
            output=Path(directory)/'capture'
            with patch.dict(os.environ,{'RUSTC':'/synthetic/unqualified/compiler'}), patch.object(sys,'argv',['capture','--output',str(output),'--capture-only']):
                with self.assertRaises(ValueError):capture.main()
            self.assertFalse(output.exists())
if __name__=='__main__':unittest.main()
