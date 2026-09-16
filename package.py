#!/usr/bin/env python3
"""Package an already-built binary without installing or publishing it."""
import argparse
import pathlib
import platform
import plistlib
import shutil
import subprocess
import struct

parser = argparse.ArgumentParser()
parser.add_argument('--debug', action='store_true')
args = parser.parse_args()
root = pathlib.Path(__file__).resolve().parent
binary = root / 'target' / ('debug' if args.debug else 'release') / 'den'
if not binary.is_file():
    parser.error('Build the matching Cargo profile first.')
dist = root / 'dist'
dist.mkdir(exist_ok=True)
if platform.system() == 'Darwin':
    bundle = dist / 'Den.app'
    executable = bundle / 'Contents' / 'MacOS' / 'den'
    executable.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(binary, executable)
    resources = bundle / 'Contents' / 'Resources'
    resources.mkdir(parents=True, exist_ok=True)
    iconset = dist / 'Den.iconset'
    icon_builder = binary.parent / 'den-icons'
    subprocess.run([str(icon_builder), str(iconset)], check=True)
    # Modern ICNS entries contain lossless PNGs. This avoids iconutil's dependency
    # on services unavailable in some build sandboxes.
    representations = [('icp4','16x16'), ('icp5','32x32'), ('icp6','32x32@2x'),
                       ('ic07','128x128'), ('ic08','256x256'), ('ic09','512x512'),
                       ('ic10','512x512@2x'), ('ic11','16x16@2x'), ('ic12','32x32@2x'),
                       ('ic13','128x128@2x'), ('ic14','256x256@2x')]
    chunks = []
    for kind, size in representations:
        png = (iconset / ('icon_' + size + '.png')).read_bytes()
        chunks.append(kind.encode('ascii') + struct.pack('>I', len(png) + 8) + png)
    payload = b''.join(chunks)
    (resources / 'Den.icns').write_bytes(b'icns' + struct.pack('>I', len(payload) + 8) + payload)
    with (bundle / 'Contents' / 'Info.plist').open('wb') as out:
        plistlib.dump({
            'CFBundleName': 'Den', 'CFBundleDisplayName': 'Den',
            'CFBundleIdentifier': 'org.den.desktop', 'CFBundleExecutable': 'den',
            'CFBundlePackageType': 'APPL', 'CFBundleShortVersionString': '0.1.0',
            'CFBundleIconFile': 'Den.icns', 'CFBundleVersion': '1', 'NSHighResolutionCapable': True,
            'LSMinimumSystemVersion': '11.0',
        }, out)
    subprocess.run(['codesign', '--force', '--deep', '--sign', '-', str(bundle)], check=True)
    print(bundle)
else:
    output = dist / 'den-linux'
    output.mkdir(exist_ok=True)
    shutil.copy2(binary, output / 'den')
    (output / 'den.desktop').write_text('[Desktop Entry]\nType=Application\nName=Den\nExec=den\nTerminal=false\nCategories=Office;\nComment=A quiet place for your work\n')
    print(output)
