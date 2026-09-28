from pathlib import Path
p=Path('.github/workflows/ci.yml'); s=p.read_text()
a='''            '{"bundle":{"createUpdaterArtifacts":false}}' |
              Set-Content -LiteralPath 'src-tauri/tauri.windows.conf.json' -Encoding utf8'''
assert a in s
s=s.replace(a,'''            $platformConfig = Get-Content -LiteralPath 'src-tauri/tauri.windows.conf.json' -Raw | ConvertFrom-Json -AsHashtable
            $platformConfig['bundle']['createUpdaterArtifacts'] = $false
            $platformConfig | ConvertTo-Json -Depth 20 |
              Set-Content -LiteralPath 'src-tauri/tauri.windows.conf.json' -Encoding utf8''')
start=s.index('  verify-csx-macos-intel:'); end=s.index('  benchmark-on-failure:',start)
job=s[start:end]
anchor='      - name: Install npm dependencies\n'
assert anchor in job
job=job.replace(anchor,'''      - uses: dtolnay/rust-toolchain@stable
      - name: Native installation, credential and launch regressions on Intel macOS
        run: cargo test --locked --manifest-path src-tauri/Cargo.toml -p deltamod-installations-domain -p deltamod-credentials-adapter -p deltamod-updater-launch-runtime -p deltamod-profile-install-runtime -p deltamod-patching-runtime
'''+anchor,1)
s=s[:start]+job+s[end:]
p.write_text(s)
print('Intel macOS native regressions added to the existing tool-validation job; Windows benchmark overrides preserve platform resources.')
