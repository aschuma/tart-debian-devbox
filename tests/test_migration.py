"""One-off converter checks. Run: python3 -m unittest discover -s tests."""
from pathlib import Path
import os
import subprocess
import tempfile
import tomllib
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'scripts/migrate-tctl-env.sh'


class MigrationTest(unittest.TestCase):
    def test_converts_both_layers_without_overwriting_or_deleting(self):
        with tempfile.TemporaryDirectory() as root:
            home = Path(root) / 'home'
            config = home / '.config/tctl'
            config.mkdir(parents=True)
            project = Path(root) / 'a project'
            project.mkdir()
            global_text = ('TCTL_SSH_USER="devbox"\n'
                           'TCTL_IDENTITY_FILE="${HOME}/.ssh/a key"\n'
                           'TCTL_LOCAL_FORWARD="5432 127.0.0.1:5432, 8080 127.0.0.1:8080"\n')
            project_text = ('TCTL_VM_NAME="demo"\nTCTL_HOST_SHARE_DIR=""\n'
                            'TCTL_ENV_PROVISION_FILES=".env,.env.local"\n'
                            'TCTL_REMOTE_FORWARD=""\n')
            (config / 'config').write_text(global_text)
            (project / '.tart-ctl-env').write_text(project_text)
            env = dict(os.environ, HOME=str(home), XDG_CONFIG_HOME=str(home / '.config'))
            for _ in range(2):
                subprocess.run(['bash', str(SCRIPT), str(project)], env=env, check=True, capture_output=True)
                global_cfg = tomllib.loads((config / 'config.toml').read_text())
                local_cfg = tomllib.loads((project / '.tart-ctl.toml').read_text())
                self.assertEqual(global_cfg['ssh']['identity_file'], str(home / '.ssh/a key'))
                self.assertEqual(len(global_cfg['ssh']['local_forward']), 2)
                self.assertEqual(local_cfg['share']['dir'], 'none')
                self.assertEqual(local_cfg['provision']['env_files'], ['.env', '.env.local'])
                self.assertEqual(local_cfg['ssh']['remote_forward'], [])
                self.assertNotIn('user', local_cfg['ssh'])
                self.assertEqual((config / 'config').read_text(), global_text)
                self.assertEqual((project / '.tart-ctl-env').read_text(), project_text)
            (project / '.tart-ctl.toml').write_text('# custom\n')
            subprocess.run(['bash', str(SCRIPT), str(project)], env=env, check=True, capture_output=True)
            self.assertEqual((project / '.tart-ctl.toml').read_text(), '# custom\n')


if __name__ == '__main__':
    unittest.main()
