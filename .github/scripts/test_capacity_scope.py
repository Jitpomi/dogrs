import copy
import unittest

from capacity_scope import closure, needs_capacity


def lock(packages):
    # Fixtures include ambiguous versions and registry source qualifiers.
    lines = ['version = 4']
    for p in packages:
        lines += ['[[package]]']
        for key, value in p.items():
            import json
            lines.append(f'{key} = {json.dumps(value)}')
    return '\n'.join(lines)


class CapacityScopeTests(unittest.TestCase):
    def setUp(self):
        self.packages = [
            {'name': 'dog-queue', 'version': '0.2.0', 'dependencies': ['redis']},
            {'name': 'hosted-system', 'version': '0.1.0', 'dependencies': ['dog-queue']},
            {'name': 'redis', 'version': '0.28.2', 'checksum': 'old'},
            {'name': 'dog-axum', 'version': '0.2.0', 'dependencies': []},
        ]
        self.before = lock(self.packages)

    def test_unrelated_lock_change_is_skipped(self):
        self.packages[-1]['dependencies'] = ['redis']
        self.assertFalse(needs_capacity(['Cargo.lock'], self.before, lock(self.packages)))

    def test_transitive_checksum_change_runs(self):
        self.packages[2]['checksum'] = 'new'
        self.assertTrue(needs_capacity(['Cargo.lock'], self.before, lock(self.packages)))

    def test_dependency_removal_runs(self):
        self.packages[0]['dependencies'] = []
        self.assertTrue(needs_capacity(['Cargo.lock'], self.before, lock(self.packages)))

    def test_dependency_addition_runs(self):
        self.packages[0]['dependencies'].append('dog-axum')
        self.assertTrue(needs_capacity(['Cargo.lock'], self.before, lock(self.packages)))

    def test_workload_paths_run(self):
        for path in ['dog-queue/src/lib.rs', 'dog-examples/hosted-system/src/capacity.rs',
                     'Cargo.toml', '.github/workflows/provider-capacity.yml',
                     '.github/scripts/capacity_scope.py']:
            with self.subTest(path=path):
                self.assertTrue(needs_capacity([path], self.before, self.before))

    def test_unrelated_source_is_skipped(self):
        self.assertFalse(needs_capacity(['dog-axum/src/lib.rs'], self.before, self.before))

    def test_version_and_source_qualified_dependencies(self):
        self.packages[0]['dependencies'] = ['redis 0.28.2 (registry+https://example.test/index)']
        self.packages[2]['source'] = 'registry+https://example.test/index'
        self.packages.append({'name': 'redis', 'version': '0.30.0'})
        self.assertEqual(len(closure(lock(self.packages))), 3)

    def test_missing_or_ambiguous_dependencies_fail_closed(self):
        for dependency in ['missing', 'redis']:
            packages = copy.deepcopy(self.packages)
            packages[0]['dependencies'] = [dependency]
            packages.append({'name': 'redis', 'version': '0.30.0'})
            with self.subTest(dependency=dependency), self.assertRaises(ValueError):
                closure(lock(packages))

    def test_missing_root_fails_closed(self):
        with self.assertRaises(ValueError):
            closure(lock(self.packages[1:]))


if __name__ == '__main__':
    unittest.main()
