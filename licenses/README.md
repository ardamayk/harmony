# Licences

Harmony is free software: you can redistribute it and/or modify it under the terms of the GNU Affero General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version. It comes with no warranty; see [LICENSE](../LICENSE).

Rust crates and npm packages are listed with their licence texts in `third-party-notices.txt`, which the Web Client serves at `/third-party-notices.txt`. Files in this folder cover the programs that run beside Harmony as separate processes and keep their own licences.

## Windows Desktop Client

The package carries the official mpv 0.41.0 release build, unmodified, which Harmony starts as its own process and drives over JSON IPC. The build's recipe is `ci/build-mingw64.sh` in the mpv source.

| Component               | Version                   | Licence                               | Text in `mpv-windows/`                    |
| ----------------------- | ------------------------- | ------------------------------------- | ----------------------------------------- |
| mpv                     | `41f6a645`                | GPL-2.0-or-later                      | `mpv-Copyright.txt`, `../GPL-2.0.txt`     |
| FFmpeg                  | `1fcb201a`, `--enable-gpl` | GPL-2.0-or-later                      | `FFmpeg-LICENSE.txt`, `../GPL-2.0.txt`    |
| libplacebo              | `47587a2d`                | LGPL-2.1-or-later                     | `libplacebo-LICENSE.txt`                  |
| FriBidi                 | 1.0.16                    | LGPL-2.1-or-later                     | `../LGPL-2.1.txt`                         |
| libiconv                | 1.18                      | LGPL-2.1-or-later                     | `../LGPL-2.1.txt`                         |
| libass                  | 21 December 2025          | ISC                                   | `libass-COPYING.txt`                      |
| dav1d                   | 21 December 2025          | BSD-2-Clause                          | `dav1d-COPYING.txt`                       |
| FreeType                | 2.14.1                    | FTL                                   | `freetype-FTL.txt`                        |
| HarfBuzz                | 12.2.0                    | MIT                                   | `harfbuzz-COPYING.txt`                    |
| Little CMS              | 21 December 2025          | MIT                                   | `lcms2-LICENSE.txt`                       |
| LuaJIT                  | 21 December 2025          | MIT                                   | `LuaJIT-COPYRIGHT.txt`                    |
| mujs                    | 1.3.8                     | ISC                                   | `mujs-COPYING.txt`                        |
| zlib                    | 1.3.1                     | Zlib                                  | `zlib-LICENSE.txt`                        |
| shaderc                 | 21 December 2025          | Apache-2.0                            | `../Apache-2.0.txt`                       |
| SPIRV-Cross             | 21 December 2025          | Apache-2.0                            | `../Apache-2.0.txt`                       |
| Vulkan Loader           | 21 December 2025          | Apache-2.0                            | `../Apache-2.0.txt`                       |
| winpthreads             | mingw-w64                 | MIT                                   | `winpthreads-COPYING.txt`                 |
| libgcc, libstdc++, libssp | GCC                     | GPL-3.0-or-later with Runtime Exception | `GCC-RUNTIME-EXCEPTION.txt`, `../GPL-3.0.txt` |

Each GitHub Release has a `-sources.tar` asset beside the package with the source of every GPL and LGPL component above except the GCC runtime, listed in `mpv-windows/sources.txt`. A component the table dates was built from its default branch on that day. libplacebo's revision is the last commit before the build; the others are read from the binaries.

## Music Server image

The image is Alpine Linux 3.21 with its `ffmpeg` package (GPL-3.0-or-later, built with `--enable-gpl`), which the Music Server runs as a separate program. `/app/licenses/alpine-packages.txt` in the image lists every installed package with its version and licence.

Alpine's source is in [aports](https://gitlab.alpinelinux.org/alpine/aports/-/tree/3.21-stable) (`community/ffmpeg` builds FFmpeg) and its upstream archives are at [distfiles.alpinelinux.org](https://distfiles.alpinelinux.org/distfiles/v3.21/).
