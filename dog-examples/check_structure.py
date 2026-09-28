"""Check the application's documented module contract, including future examples."""
from pathlib import Path
import re

root = Path(__file__).resolve().parent
errors = []
for app in sorted(root.iterdir()):
    if not (app / 'Cargo.toml').is_file():
        continue
    src = app / 'src'
    for name in ('main.rs', 'lib.rs', 'app.rs', 'hooks.rs', 'channels.rs', 'services/mod.rs', 'services/types.rs'):
        if not (src / name).is_file():
            errors.append(f'{app.name}: missing {name}')
    lib = (src / 'lib.rs').read_text() if (src / 'lib.rs').is_file() else ''
    for module in ('app', 'hooks', 'channels', 'services'):
        if not re.search(r'\bmod\s+' + module + r'\s*;', lib):
            errors.append(f'{app.name}: undeclared {module} module')
    for service in sorted((src / 'services').iterdir()):
        if not service.is_dir() or service.name == 'adapters':
            continue
        module = service / 'mod.rs'
        text = module.read_text() if module.is_file() else ''
        for suffix in ('service', 'hooks', 'shared', 'schema'):
            name = f'{service.name}_{suffix}'
            if not (service / f'{name}.rs').is_file() or not re.search(r'\bmod\s+' + name + r'\s*;', text):
                errors.append(f'{app.name}: missing/undeclared services/{service.name}/{name}.rs')
if errors:
    raise SystemExit('\n'.join(errors))
print('All DogRS example applications follow the standard module layout.')
