# Microsoft Store

OpenCheck is packaged for the Microsoft Store as an MSIX. The release
workflow makes the package next to the zip for every tag `v…`, once the
package identity is filled in.

## Version numbers

- The version is kept in one place: `version` in the root `Cargo.toml`.
  The tag (`v0.3.0`), the zip and the package follow it.
- An MSIX version has four numbers. `scripts/pack-msix.ps1` turns `0.3.0`
  into `0.3.0.0`. The fourth number is always 0: the Store keeps it for
  itself and refuses packages that use it.
- Every submission needs a higher version than the one before. A package
  that is turned down and sent again needs a new version, e.g. `0.3.1`.
- Each number is at most 65535.

## The package identity

The Store gives the app an identity, under *Product management → Product
Identity* in Partner Center. Copy its three values into
`packaging/msix/identity.json`:

| identity.json | Partner Center |
|---|---|
| `name` | Package/Identity/Name |
| `publisher` | Package/Identity/Publisher (`CN=…`) |
| `publisher_display_name` | Package/Properties/PublisherDisplayName |

Until they are filled in, releases are made without the MSIX.

## What is in the package

- `opencheck-app.exe`, the licences and the fonts' licence.
- The logos in `packaging/msix/Assets`, drawn by `scripts/app-icons.py`
  (which also writes the window icon `crates/opencheck-app/assets/icon.png`).
- `packaging/msix/AppxManifest.xml`: a desktop app (`runFullTrust`), with
  internet access for BCF Live (`internetClient`) and `.opencheck` files
  opening in OpenCheck.

The package is not signed; the Store signs it when it is published.

## Making the package by hand

On Windows with the Windows SDK (or `MAKEAPPX` pointing to `makeappx.exe`):

```powershell
cargo build --release -p opencheck-app
./scripts/pack-msix.ps1
```

## Submitting

1. Upload `OpenCheck-v<version>-x64.msix` from the GitHub release under
   *Packages* in the submission.
2. `runFullTrust` is a restricted capability: say why, e.g. "OpenCheck is a
   desktop application that reads and writes IFC and project files the
   user chooses."
3. The Store asks for a privacy policy URL, because the app can use the
   internet (BCF Live). The wiki's Architecture page on security and
   privacy can serve.

Settings and the ruleset library live in `%APPDATA%\OpenCheck`. In the
Store version Windows keeps them in the package's own folder, so the Store
and zip versions do not share them.
