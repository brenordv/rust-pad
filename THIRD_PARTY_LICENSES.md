# Third-Party Licenses

Rust-Pad is distributed under the GNU GPL v3 (see `LICENSE.md`). It also
embeds and depends on third-party components with their own licenses,
reproduced here.

This file does **not** cover transitive Cargo dependencies — those are
governed by their respective crate manifests. It lists only assets and
components bundled directly into the source tree.

---

## DejaVu Sans Mono (bundled font)

**Location:** `crates/rust-pad-ui/assets/DejaVuSansMono.ttf`
**Used by:** the Print / Export-as-PDF feature (`crates/rust-pad-ui/src/app/print/font.rs`).

The font is compiled into the binary via `include_bytes!` so the feature
produces consistent PDFs across platforms without relying on any system
font.

The full license text accompanies the font file at
`crates/rust-pad-ui/assets/DejaVuSansMono-LICENSE.txt`. A summary is
reproduced below.

> Fonts are (c) Bitstream (see below). DejaVu changes are in public domain.
> Glyphs imported from Arev fonts are (c) Tavmjong Bah (see below).
>
> **Bitstream Vera Fonts Copyright**
>
> Copyright (c) 2003 by Bitstream, Inc. All Rights Reserved. Bitstream
> Vera is a trademark of Bitstream, Inc.
>
> Permission is hereby granted, free of charge, to any person obtaining a
> copy of the fonts accompanying this license ("Fonts") and associated
> documentation files (the "Font Software"), to reproduce and distribute
> the Font Software, including without limitation the rights to use, copy,
> merge, publish, distribute, and/or sell copies of the Font Software, and
> to permit persons to whom the Font Software is furnished to do so,
> subject to the following conditions:
>
> The above copyright and trademark notices and this permission notice
> shall be included in all copies of one or more of the Font Software
> typefaces.
>
> The Font Software may be modified, altered, or added to, and in
> particular the designs of glyphs or characters in the Fonts may be
> modified and additional glyphs or characters may be added to the Fonts,
> only if the fonts are renamed to names not containing either the words
> "Bitstream" or the word "Vera".
>
> This License becomes null and void to the extent applicable to Fonts or
> Font Software that has been modified and is distributed under the
> "Bitstream Vera" names.
>
> The Font Software may be sold as part of a larger software package but
> no copy of one or more of the Font Software typefaces may be sold by
> itself.
>
> THE FONT SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND…

The full, unabridged license is available in
`crates/rust-pad-ui/assets/DejaVuSansMono-LICENSE.txt` and upstream at
<https://dejavu-fonts.github.io/License.html>.

---

## IBM Plex Sans and JetBrains Mono (bundled UI fonts, SIL OFL 1.1)

**Location:** `crates/rust-pad-ui/assets/fonts/`
**Used by:** the application UI. Both families are compiled into the binary
(registered in `App::install_fonts`) so the interface renders identically on
every platform: IBM Plex Sans for UI text, JetBrains Mono for the editor and
other monospace text.

| Font | Files | Upstream | Release tag | License file |
|------|-------|----------|-------------|--------------|
| IBM Plex Sans | Regular, Medium, SemiBold | <https://github.com/IBM/plex> | `@ibm/plex-sans@1.1.0` | `LICENSE-IBMPlexSans-OFL.txt` |
| JetBrains Mono | Regular | <https://github.com/JetBrains/JetBrainsMono> | `v2.304` | `LICENSE-JetBrainsMono-OFL.txt` |

Copyright notices, as declared in the license files:

> Copyright © 2017 IBM Corp. with Reserved Font Name "Plex"

> Copyright 2020 The JetBrains Mono Project Authors
> (<https://github.com/JetBrains/JetBrainsMono>)

Both families are licensed under the **SIL Open Font License, Version 1.1**
(<https://scripts.sil.org/OFL>). In brief: the fonts may be used, bundled,
embedded, redistributed, and sold with any software; they may not be sold on
their own; copyright and license notices must accompany every copy; and a
Reserved Font Name (IBM Plex declares "Plex"; JetBrains Mono declares none)
may only be used by unmodified versions, so any modified build must be
renamed. The fonts bundled here are unmodified upstream copies.

The full license texts ship next to the font files (paths in the table above)
and are viewable in the app under Help > About > Third-Party Notices. File
integrity records (SHA-256 per file, plus the update procedure) live in
`crates/rust-pad-ui/assets/fonts/MANIFEST.md`.
