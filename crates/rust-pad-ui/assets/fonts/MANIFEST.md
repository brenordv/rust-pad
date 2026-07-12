# Bundled fonts

Both families are licensed under the SIL Open Font License 1.1; the license
texts ship alongside this file and are shown in the app's About dialog.
The files are unmodified copies from the upstream release archives listed
below, verified by SHA-256 at vendoring time.

## IBM Plex Sans

- Source: https://github.com/IBM/plex
- Release tag: `@ibm/plex-sans@1.1.0` (asset `ibm-plex-sans.zip`, path `fonts/complete/ttf/`)
- License: `LICENSE-IBMPlexSans-OFL.txt`

| File | SHA-256 |
|---|---|
| IBMPlexSans-Regular.ttf | 975DCDA37D80F038DCD143C22E33CA2D97A0CC5A929AACE1C749153B0FE1AFA5 |
| IBMPlexSans-Medium.ttf | 331C8639D7598B2CDE62A911A71DB195E30CB655CD6BDF2E324A7E984955F907 |
| IBMPlexSans-SemiBold.ttf | A20CAF8286023A6A7A85E40B1D2A4AE9FC3E3B1F9EDA8F4C542DD4986AF67BB1 |

## JetBrains Mono

- Source: https://github.com/JetBrains/JetBrainsMono
- Release tag: `v2.304` (asset `JetBrainsMono-2.304.zip`, path `fonts/ttf/`)
- License: `LICENSE-JetBrainsMono-OFL.txt`

| File | SHA-256 |
|---|---|
| JetBrainsMono-Regular.ttf | A0BF60EF0F83C5ED4D7A75D45838548B1F6873372DFAC88F71804491898D138F |

## Updating

Download the new release archive from the canonical repository over HTTPS,
replace the TTFs, recompute the SHA-256 values (`Get-FileHash -Algorithm
SHA256`), and update this table plus the release tag in the same commit.
