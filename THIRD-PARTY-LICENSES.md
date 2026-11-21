# Third-party software

Wisp Player is built from the 434 Rust crates listed below (the ones that go into the Windows, Linux and Android builds, as reported by `cargo tree`; the last column says which). Each crate is distributed under the license shown. Where a choice is offered (`MIT OR Apache-2.0`), Wisp Player uses it under either one; for Slint the choice is the GNU GPL v3, which is why Wisp Player itself is released under the GPL v3 (see [LICENSE](LICENSE)). The full text of each license is in the crate's source package on crates.io.

Also used, and not Rust crates:

* The Android build draws with Google's Skia graphics library (BSD 3-Clause), prebuilt by the `skia-bindings` crate together with the libraries Skia bundles (FreeType, HarfBuzz, ICU, libpng, libjpeg-turbo, zlib, Expat and others, each under its own permissive license). Linux and Windows builds do not contain it.
* The Flatpak runs on the Freedesktop runtime, which carries its own licenses, and ships none of it itself.
* The icons in the interface are Material Design icons by Google, under the Apache License 2.0.
* On request only, the track editor asks [MusicBrainz](https://musicbrainz.org) (its core data is CC0), the [Cover Art Archive](https://coverartarchive.org) and [LRCLIB](https://lrclib.net) for matches, covers and lyrics. Wisp Player does not include any of that data. Covers and lyrics belong to their owners: use what you save for your own listening.

| Crate | Version | License | Source | Used on |
| --- | --- | --- | --- | --- |
| ab_glyph | 0.2.32 | Apache-2.0 | https://github.com/alexheretic/ab-glyph | Linux |
| ab_glyph_rasterizer | 0.1.10 | Apache-2.0 | https://github.com/alexheretic/ab-glyph | Linux |
| accesskit | 0.25.2 | MIT OR Apache-2.0 | https://github.com/AccessKit/accesskit | Linux, Windows |
| accesskit_atspi_common | 0.21.1 | MIT OR Apache-2.0 | https://github.com/AccessKit/accesskit | Linux |
| accesskit_consumer | 0.39.2 | MIT OR Apache-2.0 | https://github.com/AccessKit/accesskit | Linux, Windows |
| accesskit_unix | 0.24.1 | MIT OR Apache-2.0 | https://github.com/AccessKit/accesskit | Linux |
| accesskit_windows | 0.35.2 | MIT OR Apache-2.0 | https://github.com/AccessKit/accesskit | Windows |
| accesskit_winit | 0.34.2 | Apache-2.0 | https://github.com/AccessKit/accesskit | Linux, Windows |
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 | https://github.com/oyvindln/adler2 | Android, Linux, Windows |
| ahash | 0.8.12 | MIT OR Apache-2.0 | https://github.com/tkaitchuck/ahash | Linux |
| allocator-api2 | 0.2.21 | MIT OR Apache-2.0 | https://github.com/zakarumych/allocator-api2 | Android, Linux, Windows |
| alsa | 0.11.0 | Apache-2.0/MIT | https://github.com/diwic/alsa-rs | Linux |
| alsa-sys | 0.4.0 | MIT | https://github.com/diwic/alsa-sys | Linux |
| android_system_properties | 0.1.6 | MIT OR Apache-2.0 | https://github.com/nical/android_system_properties | Android |
| android-activity | 0.6.1 | MIT OR Apache-2.0 | https://github.com/rust-mobile/android-activity | Android |
| android-properties | 0.2.2 | MIT | https://github.com/miklelappo/android-properties | Android |
| annotate-snippets | 0.12.18 | MIT OR Apache-2.0 | https://github.com/rust-lang/annotate-snippets-rs | Android, Linux, Windows |
| anstyle | 1.0.14 | MIT OR Apache-2.0 | https://github.com/rust-cli/anstyle.git | Android, Linux, Windows |
| arboard | 3.7.0 | MIT OR Apache-2.0 | https://github.com/1Password/arboard | Linux, Windows |
| arrayref | 0.3.9 | BSD-2-Clause | https://github.com/droundy/arrayref | Android, Linux, Windows |
| arrayvec | 0.7.8 | MIT OR Apache-2.0 | https://github.com/bluss/arrayvec | Android, Linux, Windows |
| as-raw-xcb-connection | 1.0.1 | MIT OR Apache-2.0 | https://github.com/psychon/as-raw-xcb-connection | Linux |
| async-broadcast | 0.7.2 | MIT OR Apache-2.0 | https://github.com/smol-rs/async-broadcast | Linux |
| async-channel | 2.5.0 | Apache-2.0 OR MIT | https://github.com/smol-rs/async-channel | Linux |
| async-executor | 1.14.1 | Apache-2.0 OR MIT | https://github.com/smol-rs/async-executor | Linux |
| async-io | 2.6.0 | Apache-2.0 OR MIT | https://github.com/smol-rs/async-io | Linux |
| async-lock | 3.5.0 | Apache-2.0 OR MIT | https://github.com/smol-rs/async-lock | Linux |
| async-process | 2.5.0 | Apache-2.0 OR MIT | https://github.com/smol-rs/async-process | Linux |
| async-recursion | 1.2.0 | MIT OR Apache-2.0 | https://github.com/dcchut/async-recursion | Linux |
| async-signal | 0.2.14 | Apache-2.0 OR MIT | https://github.com/smol-rs/async-signal | Linux |
| async-task | 4.7.1 | Apache-2.0 OR MIT | https://github.com/smol-rs/async-task | Linux |
| async-trait | 0.1.92 | MIT OR Apache-2.0 | https://github.com/dtolnay/async-trait | Linux |
| atomic-waker | 1.1.2 | Apache-2.0 OR MIT | https://github.com/smol-rs/atomic-waker | Linux |
| atspi | 0.29.0 | Apache-2.0 OR MIT | https://github.com/odilia-app/atspi | Linux |
| atspi-common | 0.13.0 | Apache-2.0 OR MIT | https://github.com/odilia-app/atspi | Linux |
| atspi-proxies | 0.13.0 | Apache-2.0 OR MIT | https://github.com/odilia-app/atspi | Linux |
| audioadapter | 5.0.0 | MIT OR Apache-2.0 | https://github.com/HEnquist/audioadapter-rs | Android, Linux, Windows |
| audioadapter-buffers | 5.2.0 | MIT OR Apache-2.0 | https://github.com/HEnquist/audioadapter-buffers-rs | Android, Linux, Windows |
| audioadapter-sample | 5.2.0 | MIT OR Apache-2.0 | https://github.com/HEnquist/audioadapter-sample-rs | Android, Linux, Windows |
| audio-codec-algorithms | 0.8.1 | 0BSD OR Apache-2.0 | https://github.com/karip/audio-codec-algorithms | Android, Linux, Windows |
| base64 | 0.23.1 | MIT OR Apache-2.0 | https://github.com/marshallpierce/rust-base64 | Android, Linux, Windows |
| base64ct | 1.8.3 | Apache-2.0 OR MIT | https://github.com/RustCrypto/formats | Windows |
| bitflags | 2.13.2 | MIT OR Apache-2.0 | https://github.com/bitflags/bitflags | Android, Linux, Windows |
| blocking | 1.7.0 | Apache-2.0 OR MIT | https://github.com/smol-rs/blocking | Linux |
| by_address | 1.2.1 | MIT OR Apache-2.0 | https://github.com/mbrubeck/by_address | Android, Linux, Windows |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT | https://github.com/Lokathor/bytemuck | Android, Linux, Windows |
| bytemuck_derive | 1.12.1 | Zlib OR Apache-2.0 OR MIT | https://github.com/Lokathor/bytemuck | Android, Linux, Windows |
| byteorder-lite | 0.1.0 | Unlicense OR MIT | https://github.com/image-rs/byteorder-lite | Android, Linux, Windows |
| bytes | 1.12.2 | MIT | https://github.com/tokio-rs/bytes | Android, Linux, Windows |
| calloop | 0.13.0 | MIT | https://github.com/Smithay/calloop | Linux |
| calloop | 0.14.5 | MIT | https://github.com/Smithay/calloop | Linux |
| calloop-wayland-source | 0.3.0 | MIT | https://github.com/smithay/calloop-wayland-source | Linux |
| calloop-wayland-source | 0.4.1 | MIT | https://github.com/smithay/calloop-wayland-source | Linux |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 | https://github.com/rust-lang/cfg-if | Android, Linux, Windows |
| chrono | 0.4.45 | MIT OR Apache-2.0 | https://github.com/chronotope/chrono | Android, Linux, Windows |
| clipboard-win | 5.4.3 | BSL-1.0 | https://github.com/DoumanAsh/clipboard-win | Windows |
| clru | 0.6.3 | MIT | https://github.com/marmeladema/clru-rs | Android, Linux, Windows |
| color_quant | 1.1.0 | MIT | https://github.com/image-rs/color_quant.git | Android, Linux, Windows |
| combine | 4.6.9 | MIT | https://github.com/Marwes/combine | Android |
| concurrent-queue | 2.5.0 | Apache-2.0 OR MIT | https://github.com/smol-rs/concurrent-queue | Linux |
| const-field-offset | 0.2.1 | MIT OR Apache-2.0 | https://github.com/slint-ui/slint | Android, Linux, Windows |
| const-field-offset-macro | 0.2.1 | MIT OR Apache-2.0 | https://github.com/slint-ui/slint | Android, Linux, Windows |
| convert_case | 0.10.0 | MIT | https://github.com/rutrum/convert-case | Android, Linux, Windows |
| countme | 3.0.1 | MIT OR Apache-2.0 | https://github.com/matklad/countme | Android, Linux, Windows |
| cpal | 0.18.2 | Apache-2.0 | https://github.com/RustAudio/cpal | Android, Linux, Windows |
| crc32fast | 1.5.5 | MIT OR Apache-2.0 | https://github.com/srijs/rust-crc32fast | Android, Linux, Windows |
| critical-section | 1.2.0 | MIT OR Apache-2.0 | https://github.com/rust-embedded/critical-section | Android, Linux, Windows |
| crossbeam-channel | 0.5.17 | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam | Windows |
| crossbeam-deque | 0.8.8 | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam | Android, Linux, Windows |
| crossbeam-epoch | 0.9.21 | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam | Android, Linux, Windows |
| crossbeam-queue | 0.3.14 | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam | Android, Linux, Windows |
| crossbeam-utils | 0.8.23 | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam | Android, Linux, Windows |
| ctor | 0.10.1 | Apache-2.0 OR MIT | https://github.com/mmastrac/rust-ctor | Linux |
| cursor-icon | 1.2.0 | MIT OR Apache-2.0 OR Zlib | https://github.com/rust-windowing/cursor-icon | Linux, Windows |
| dasp_sample | 0.11.0 | MIT OR Apache-2.0 | https://github.com/rustaudio/sample.git | Android, Linux, Windows |
| data-url | 0.3.2 | MIT OR Apache-2.0 | https://github.com/servo/rust-url | Android, Linux, Windows |
| der | 0.8.3 | Apache-2.0 OR MIT | https://github.com/RustCrypto/formats | Windows |
| derive_more | 2.1.1 | MIT | https://github.com/JelteF/derive_more | Android, Linux, Windows |
| derive_more-impl | 2.1.1 | MIT | https://github.com/JelteF/derive_more | Android, Linux, Windows |
| displaydoc | 0.2.7 | MIT OR Apache-2.0 | https://github.com/yaahc/displaydoc | Android, Linux, Windows |
| dlib | 0.5.3 | MIT | https://github.com/elinorbgr/dlib | Linux |
| downcast-rs | 1.2.1 | MIT/Apache-2.0 | https://github.com/marcianx/downcast-rs | Linux |
| dpi | 0.1.2 | Apache-2.0 AND MIT | https://github.com/rust-windowing/winit | Linux, Windows |
| either | 1.19.0 | MIT OR Apache-2.0 | https://github.com/rayon-rs/either | Android, Linux, Windows |
| endi | 1.1.1 | MIT | https://github.com/zeenix/endi | Linux |
| enumflags2 | 0.7.12 | MIT OR Apache-2.0 | https://github.com/meithecatte/enumflags2 | Linux |
| enumflags2_derive | 0.7.12 | MIT OR Apache-2.0 | https://github.com/meithecatte/enumflags2 | Linux |
| equivalent | 1.0.3 | Apache-2.0 OR MIT | https://github.com/indexmap-rs/equivalent | Android, Linux, Windows |
| errno | 0.3.15 | MIT OR Apache-2.0 | https://github.com/lambda-fairy/rust-errno | Linux |
| error-code | 3.4.0 | BSL-1.0 | https://github.com/DoumanAsh/error-code | Windows |
| euclid | 0.22.14 | MIT OR Apache-2.0 | https://github.com/servo/euclid | Android, Linux, Windows |
| event-listener | 5.4.2 | Apache-2.0 OR MIT | https://github.com/smol-rs/event-listener | Linux |
| event-listener-strategy | 0.5.4 | Apache-2.0 OR MIT | https://github.com/smol-rs/event-listener-strategy | Linux |
| extended | 0.1.0 | MIT | https://github.com/depp/extended-rs | Android, Linux, Windows |
| fastrand | 2.5.0 | Apache-2.0 OR MIT | https://github.com/smol-rs/fastrand | Android, Linux, Windows |
| fdeflate | 0.3.7 | MIT OR Apache-2.0 | https://github.com/image-rs/fdeflate | Android, Linux, Windows |
| field-offset | 0.3.6 | MIT OR Apache-2.0 | https://github.com/Diggsey/rust-field-offset | Android, Linux, Windows |
| fixed_decimal | 0.7.3 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| flate2 | 1.1.11 | MIT OR Apache-2.0 | https://github.com/rust-lang/flate2-rs | Android, Linux, Windows |
| float-cmp | 0.9.0 | MIT | https://github.com/mikedilger/float-cmp | Android, Linux, Windows |
| foldhash | 0.2.0 | Zlib | https://github.com/orlp/foldhash | Android, Linux, Windows |
| fontdb | 0.24.0 | MIT | https://github.com/RazrFalcon/fontdb | Android, Linux, Windows |
| fontique | 0.12.0 | Apache-2.0 OR MIT | https://github.com/linebender/parley | Android, Linux, Windows |
| font-types | 0.12.7 | MIT OR Apache-2.0 | https://github.com/googlefonts/fontations | Android, Linux, Windows |
| form_urlencoded | 1.2.2 | MIT OR Apache-2.0 | https://github.com/servo/rust-url | Android, Linux, Windows |
| futures | 0.3.34 | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs | Linux |
| futures-channel | 0.3.34 | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs | Linux |
| futures-core | 0.3.34 | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs | Linux |
| futures-executor | 0.3.34 | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs | Linux |
| futures-io | 0.3.34 | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs | Linux |
| futures-lite | 2.6.1 | Apache-2.0 OR MIT | https://github.com/smol-rs/futures-lite | Linux |
| futures-macro | 0.3.34 | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs | Linux |
| futures-sink | 0.3.34 | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs | Linux |
| futures-task | 0.3.34 | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs | Linux |
| futures-util | 0.3.34 | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs | Linux |
| gethostname | 1.1.0 | Apache-2.0 | https://codeberg.org/swsnr/gethostname.rs.git | Linux |
| getrandom | 0.2.17 | MIT OR Apache-2.0 | https://github.com/rust-random/getrandom | Android, Linux |
| getrandom | 0.3.4 | MIT OR Apache-2.0 | https://github.com/rust-random/getrandom | Linux |
| gif | 0.14.2 | MIT OR Apache-2.0 | https://github.com/image-rs/image-gif | Android, Linux, Windows |
| glow | 0.18.0 | MIT OR Apache-2.0 OR Zlib | https://github.com/grovesNL/glow | Android |
| glutin | 0.32.3 | Apache-2.0 | https://github.com/rust-windowing/glutin | Android |
| glutin_egl_sys | 0.7.1 | Apache-2.0 | https://github.com/rust-windowing/glutin | Android |
| harfrust | 0.12.0 | MIT | https://github.com/harfbuzz/harfrust | Android, Linux, Windows |
| hashbrown | 0.14.5 | MIT OR Apache-2.0 | https://github.com/rust-lang/hashbrown | Android, Linux, Windows |
| hashbrown | 0.16.1 | MIT OR Apache-2.0 | https://github.com/rust-lang/hashbrown | Android, Linux, Windows |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | https://github.com/rust-lang/hashbrown | Android, Linux, Windows |
| heck | 0.5.0 | MIT OR Apache-2.0 | https://github.com/withoutboats/heck | Android, Linux, Windows |
| hex | 0.4.3 | MIT OR Apache-2.0 | https://github.com/KokaKiwi/rust-hex | Linux |
| htmlparser | 0.2.1 | MIT OR Apache-2.0 | https://github.com/jdrouet/htmlparser.git | Android, Linux, Windows |
| http | 1.5.0 | MIT OR Apache-2.0 | https://github.com/hyperium/http | Android, Linux, Windows |
| httparse | 1.10.1 | MIT OR Apache-2.0 | https://github.com/seanmonstar/httparse | Android, Linux, Windows |
| iana-time-zone | 0.1.65 | MIT OR Apache-2.0 | https://github.com/strawlab/iana-time-zone | Android, Linux |
| icu_collections | 2.4.0 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| icu_decimal | 2.4.0 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| icu_decimal_data | 2.4.0 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| icu_locale_core | 2.4.0 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| icu_locale_fallback | 2.4.0 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| icu_locale_fallback_data | 2.4.0 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| icu_normalizer | 2.4.0 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| icu_normalizer_data | 2.4.0 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| icu_properties | 2.4.0 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| icu_properties_data | 2.4.0 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| icu_provider | 2.4.0 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| icu_segmenter | 2.4.0 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| icu_segmenter_data | 2.4.0 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| idna | 1.1.0 | MIT OR Apache-2.0 | https://github.com/servo/rust-url/ | Android, Linux, Windows |
| idna_adapter | 1.2.2 | Apache-2.0 OR MIT | https://github.com/hsivonen/idna_adapter | Android, Linux, Windows |
| image | 0.25.10 | MIT OR Apache-2.0 | https://github.com/image-rs/image | Android, Linux, Windows |
| imagesize | 0.15.0 | MIT | https://github.com/Roughsketch/imagesize | Android, Linux, Windows |
| image-webp | 0.2.4 | MIT OR Apache-2.0 | https://github.com/image-rs/image-webp | Android, Linux, Windows |
| imgref | 1.12.3 | CC0-1.0 OR Apache-2.0 | https://github.com/kornelski/imgref | Linux, Windows |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | https://github.com/indexmap-rs/indexmap | Android, Linux, Windows |
| i-slint-backend-android-activity | 1.19.0 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | https://github.com/slint-ui/slint | Android |
| i-slint-backend-selector | 1.19.0 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | https://github.com/slint-ui/slint | Android, Linux, Windows |
| i-slint-backend-winit | 1.19.0 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | https://github.com/slint-ui/slint | Linux, Windows |
| i-slint-common | 1.19.0 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | https://github.com/slint-ui/slint | Android, Linux, Windows |
| i-slint-compiler | 1.19.0 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | https://github.com/slint-ui/slint | Android, Linux, Windows |
| i-slint-core | 1.19.0 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | https://github.com/slint-ui/slint | Android, Linux, Windows |
| i-slint-core-macros | 1.19.0 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | https://github.com/slint-ui/slint | Android, Linux, Windows |
| i-slint-renderer-skia | 1.19.0 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | https://github.com/slint-ui/slint | Android |
| i-slint-renderer-software | 1.19.0 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | https://github.com/slint-ui/slint | Linux, Windows |
| itertools | 0.15.0 | MIT OR Apache-2.0 | https://github.com/rust-itertools/itertools | Android, Linux, Windows |
| itoa | 1.0.19 | MIT OR Apache-2.0 | https://github.com/dtolnay/itoa | Android, Linux, Windows |
| jni | 0.22.4 | MIT OR Apache-2.0 | https://github.com/jni-rs/jni-rs | Android |
| jni-macros | 0.22.4 | MIT OR Apache-2.0 | https://github.com/jni-rs/jni-rs | Android |
| jni-sys | 0.3.1 | MIT OR Apache-2.0 | https://github.com/jni-rs/jni-sys | Android |
| jni-sys | 0.4.1 | MIT OR Apache-2.0 | https://github.com/jni-rs/jni-sys | Android |
| jni-sys-macros | 0.4.1 | MIT OR Apache-2.0 | https://github.com/jni-rs/jni-sys | Android |
| jpeg-decoder | 0.3.2 | MIT OR Apache-2.0 | https://github.com/image-rs/jpeg-decoder | Android, Linux, Windows |
| keyboard-types | 0.8.3 | MIT OR Apache-2.0 | https://github.com/rust-windowing/keyboard-types | Windows |
| kurbo | 0.13.1 | Apache-2.0 OR MIT | https://github.com/linebender/kurbo | Android, Linux, Windows |
| lazy_static | 1.5.1 | MIT OR Apache-2.0 | https://github.com/rust-lang-nursery/lazy-static.rs | Android, Linux, Windows |
| libc | 0.2.191 | MIT OR Apache-2.0 | https://github.com/rust-lang/libc | Android, Linux |
| libloading | 0.8.9 | ISC | https://github.com/nagisa/rust_libloading/ | Android, Linux |
| libm | 0.2.16 | MIT | https://github.com/rust-lang/compiler-builtins | Android, Linux, Windows |
| linebender_resource_handle | 0.1.1 | Apache-2.0 OR MIT | https://github.com/linebender/raw_resource_handle | Android, Linux, Windows |
| linux-raw-sys | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | https://github.com/sunfishcode/linux-raw-sys | Linux |
| linux-raw-sys | 0.4.15 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | https://github.com/sunfishcode/linux-raw-sys | Linux |
| litemap | 0.8.3 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| lock_api | 0.4.14 | MIT OR Apache-2.0 | https://github.com/Amanieu/parking_lot | Linux |
| log | 0.4.35 | MIT OR Apache-2.0 | https://github.com/rust-lang/log | Android, Linux, Windows |
| lyon_algorithms | 1.0.22 | MIT OR Apache-2.0 | https://github.com/nical/lyon | Android, Linux, Windows |
| lyon_extra | 1.1.0 | MIT OR Apache-2.0 | https://github.com/nical/lyon | Android, Linux, Windows |
| lyon_geom | 1.0.20 | MIT OR Apache-2.0 | https://github.com/nical/lyon | Android, Linux, Windows |
| lyon_path | 1.0.19 | MIT OR Apache-2.0 | https://github.com/nical/lyon | Android, Linux, Windows |
| memchr | 2.8.3 | Unlicense OR MIT | https://github.com/BurntSushi/memchr | Android, Linux, Windows |
| memmap2 | 0.9.12 | MIT OR Apache-2.0 | https://github.com/RazrFalcon/memmap2-rs | Android, Linux, Windows |
| memoffset | 0.9.1 | MIT | https://github.com/Gilnaa/memoffset | Android, Linux, Windows |
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 | https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide | Android, Linux, Windows |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 | https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide | Android, Linux, Windows |
| moxcms | 0.8.1 | BSD-3-Clause OR Apache-2.0 | https://github.com/awxkee/moxcms.git | Android, Linux, Windows |
| muda | 0.21.2 | Apache-2.0 OR MIT | https://github.com/tauri-apps/muda | Windows |
| native-tls | 0.2.18 | MIT OR Apache-2.0 | https://github.com/rust-native-tls/rust-native-tls | Windows |
| natord | 1.0.9 | MIT | https://github.com/lifthrasiir/rust-natord | Android, Linux, Windows |
| ndk | 0.9.0 | MIT OR Apache-2.0 | https://github.com/rust-mobile/ndk | Android |
| ndk-context | 0.1.1 | MIT OR Apache-2.0 | https://github.com/rust-windowing/android-ndk-rs | Android |
| ndk-sys | 0.6.0+11769913 | MIT OR Apache-2.0 | https://github.com/rust-mobile/ndk | Android |
| num_enum | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | https://github.com/illicitonion/num_enum | Android, Linux, Windows |
| num_enum_derive | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | https://github.com/illicitonion/num_enum | Android, Linux, Windows |
| num-complex | 0.4.6 | MIT OR Apache-2.0 | https://github.com/rust-num/num-complex | Android, Linux, Windows |
| num-derive | 0.4.2 | MIT OR Apache-2.0 | https://github.com/rust-num/num-derive | Android |
| num-integer | 0.1.47 | MIT OR Apache-2.0 | https://github.com/rust-num/num-integer | Android, Linux, Windows |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | https://github.com/rust-num/num-traits | Android, Linux, Windows |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | https://github.com/matklad/once_cell | Android, Linux, Windows |
| opus-decoder | 0.1.1 | MIT OR Apache-2.0 | https://github.com/TadeuszWolfGang/Rusopus | Android, Linux, Windows |
| ordered-stream | 0.2.0 | MIT OR Apache-2.0 | https://github.com/danieldg/ordered-stream | Linux |
| owned_ttf_parser | 0.25.1 | Apache-2.0 | https://github.com/alexheretic/owned-ttf-parser | Linux |
| parking | 2.2.1 | Apache-2.0 OR MIT | https://github.com/smol-rs/parking | Linux |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 | https://github.com/Amanieu/parking_lot | Linux |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 | https://github.com/Amanieu/parking_lot | Linux |
| parlance | 0.1.1 | Apache-2.0 OR MIT | https://github.com/linebender/parley | Android, Linux, Windows |
| parley | 0.12.0 | Apache-2.0 OR MIT | https://github.com/linebender/parley | Android, Linux, Windows |
| parley_data | 0.12.0 | Apache-2.0 OR MIT | https://github.com/linebender/parley | Android, Linux, Windows |
| parley_emoji | 0.12.0 | MIT | https://github.com/linebender/parley | Android, Linux, Windows |
| parley_engine | 0.12.0 | Apache-2.0 OR MIT | https://github.com/linebender/parley | Android, Linux, Windows |
| pem-rfc7468 | 1.0.0 | Apache-2.0 OR MIT | https://github.com/RustCrypto/formats | Windows |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 | https://github.com/servo/rust-url/ | Android, Linux, Windows |
| phf | 0.14.0 | MIT | https://github.com/rust-phf/rust-phf | Linux |
| phf_generator | 0.14.0 | MIT | https://github.com/rust-phf/rust-phf | Linux |
| phf_macros | 0.14.0 | MIT | https://github.com/rust-phf/rust-phf | Linux |
| phf_shared | 0.14.0 | MIT | https://github.com/rust-phf/rust-phf | Linux |
| pico-args | 0.5.0 | MIT | https://github.com/RazrFalcon/pico-args | Android, Linux, Windows |
| pin-project | 1.2.0 | Apache-2.0 OR MIT | https://github.com/taiki-e/pin-project | Android, Linux, Windows |
| pin-project-internal | 1.2.0 | Apache-2.0 OR MIT | https://github.com/taiki-e/pin-project | Android, Linux, Windows |
| pin-project-lite | 0.2.18 | Apache-2.0 OR MIT | https://github.com/taiki-e/pin-project-lite | Linux, Windows |
| pin-utils | 0.1.1 | MIT OR Apache-2.0 | https://github.com/rust-lang/pin-utils | Android, Linux, Windows |
| pin-weak | 1.1.0 | MIT | https://github.com/sixtyfpsui/pin-weak | Android, Linux, Windows |
| piper | 0.2.5 | MIT OR Apache-2.0 | https://github.com/smol-rs/piper | Linux |
| png | 0.18.1 | MIT OR Apache-2.0 | https://github.com/image-rs/image-png | Android, Linux, Windows |
| polling | 3.11.0 | Apache-2.0 OR MIT | https://github.com/smol-rs/polling | Linux |
| pollster | 1.0.3 | MIT OR Apache-2.0 | https://github.com/zesterer/pollster | Linux |
| polycool | 0.4.0 | MIT OR Apache-2.0 | https://github.com/linebender/kurbo | Android, Linux, Windows |
| portable-atomic | 1.15.0 | Apache-2.0 OR MIT | https://github.com/taiki-e/portable-atomic | Android, Linux, Windows |
| potential_utf | 0.1.6 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| primal-check | 0.3.4 | MIT OR Apache-2.0 | https://github.com/huonw/primal | Android, Linux, Windows |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | https://github.com/dtolnay/proc-macro2 | Android, Linux, Windows |
| proc-macro-crate | 3.5.0 | MIT OR Apache-2.0 | https://github.com/bkchr/proc-macro-crate | Android, Linux, Windows |
| pulldown-cmark | 0.13.6 | MIT | https://github.com/raphlinus/pulldown-cmark | Android, Linux, Windows |
| pxfm | 0.1.30 | BSD-3-Clause OR Apache-2.0 | https://github.com/awxkee/pxfm | Android, Linux, Windows |
| quick-error | 2.0.1 | MIT/Apache-2.0 | http://github.com/tailhook/quick-error | Android, Linux, Windows |
| quick-xml | 0.41.0 | MIT | https://github.com/tafia/quick-xml | Linux |
| quote | 1.0.47 | MIT OR Apache-2.0 | https://github.com/dtolnay/quote | Android, Linux, Windows |
| raw-window-handle | 0.6.2 | MIT OR Apache-2.0 OR Zlib | https://github.com/rust-windowing/raw-window-handle | Android, Linux, Windows |
| rayon | 1.12.0 | MIT OR Apache-2.0 | https://github.com/rayon-rs/rayon | Android, Linux, Windows |
| rayon-core | 1.13.0 | MIT OR Apache-2.0 | https://github.com/rayon-rs/rayon | Android, Linux, Windows |
| read-fonts | 0.41.0 | MIT OR Apache-2.0 | https://github.com/googlefonts/fontations | Android, Linux, Windows |
| realfft | 3.5.1 | MIT | https://github.com/HEnquist/realfft | Android, Linux, Windows |
| redb | 5.0.0 | MIT OR Apache-2.0 | https://github.com/cberner/redb | Android, Linux, Windows |
| regex-lite | 0.1.9 | MIT OR Apache-2.0 | https://github.com/rust-lang/regex | Android, Linux, Windows |
| resvg | 0.49.0 | Apache-2.0 OR MIT | https://github.com/linebender/resvg | Android, Linux, Windows |
| rfd | 0.18.0 | MIT | https://github.com/PolyMeilex/rfd | Linux, Windows |
| rgb | 0.8.53 | MIT | https://github.com/kornelski/rust-rgb | Android, Linux, Windows |
| ring | 0.17.14 | Apache-2.0 AND ISC | https://github.com/briansmith/ring | Android, Linux |
| rowan | 0.17.0 | MIT OR Apache-2.0 | https://github.com/rust-analyzer/rowan | Android, Linux, Windows |
| roxmltree | 0.21.2 | MIT OR Apache-2.0 | https://github.com/RazrFalcon/roxmltree | Android, Linux, Windows |
| rspolib | 0.1.2 | MIT | https://github.com/mondeja/rspolib | Android, Linux, Windows |
| rubato | 5.1.0 | MIT OR Apache-2.0 | https://github.com/HEnquist/rubato | Android, Linux, Windows |
| rustc-hash | 1.1.0 | Apache-2.0/MIT | https://github.com/rust-lang-nursery/rustc-hash | Android, Linux, Windows |
| rustfft | 6.4.1 | MIT OR Apache-2.0 | https://github.com/ejmahler/RustFFT | Android, Linux, Windows |
| rustix | 0.38.44 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | https://github.com/bytecodealliance/rustix | Linux |
| rustix | 1.1.6 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | https://github.com/bytecodealliance/rustix | Linux |
| rustls | 0.23.46 | Apache-2.0 OR ISC OR MIT | https://github.com/rustls/rustls | Android, Linux |
| rustls-pki-types | 1.16.0 | MIT OR Apache-2.0 | https://github.com/rustls/pki-types | Android, Linux, Windows |
| rustls-webpki | 0.103.17 | ISC | https://github.com/rustls/webpki | Android, Linux |
| rustversion | 1.0.23 | MIT OR Apache-2.0 | https://github.com/dtolnay/rustversion | Android, Linux, Windows |
| schannel | 0.1.30 | MIT | https://github.com/steffengy/schannel-rs | Windows |
| scoped-tls | 1.0.1 | MIT/Apache-2.0 | https://github.com/alexcrichton/scoped-tls | Linux |
| scoped-tls-hkt | 0.1.5 | MIT/Apache-2.0 | https://github.com/Diggsey/scoped-tls-hkt | Android, Linux, Windows |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 | https://github.com/bluss/scopeguard | Android, Linux, Windows |
| sctk-adwaita | 0.10.1 | MIT | https://github.com/PolyMeilex/sctk-adwaita | Linux |
| serde | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde | Android, Linux, Windows |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde | Android, Linux, Windows |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde | Android, Linux, Windows |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | https://github.com/serde-rs/json | Android, Linux, Windows |
| serde_repr | 0.1.21 | MIT OR Apache-2.0 | https://github.com/dtolnay/serde-repr | Linux |
| signal-hook-registry | 1.4.8 | MIT OR Apache-2.0 | https://github.com/vorner/signal-hook | Linux |
| simd_cesu8 | 1.2.0 | Apache-2.0 OR MIT | https://github.com/seancroach/simd_cesu8 | Android |
| simd-adler32 | 0.3.10 | MIT | https://github.com/mcountryman/simd-adler32 | Android, Linux, Windows |
| simdutf8 | 0.1.5 | MIT OR Apache-2.0 | https://github.com/rusticstuff/simdutf8 | Android |
| simplecss | 0.2.2 | Apache-2.0 OR MIT | https://github.com/linebender/simplecss | Android, Linux, Windows |
| siphasher | 1.0.4 | MIT OR Apache-2.0 | https://github.com/jedisct1/rust-siphash | Android, Linux, Windows |
| skia-bindings | 0.157.0 | MIT | https://github.com/rust-skia/rust-skia | Android |
| skia-safe | 0.157.0 | MIT | https://github.com/rust-skia/rust-skia | Android |
| skrifa | 0.44.0 | MIT OR Apache-2.0 | https://github.com/googlefonts/fontations | Android, Linux, Windows |
| slab | 0.4.12 | MIT | https://github.com/tokio-rs/slab | Android, Linux, Windows |
| slint | 1.19.0 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | https://github.com/slint-ui/slint | Android, Linux, Windows |
| slint-macros | 1.19.0 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | https://github.com/slint-ui/slint | Android, Linux, Windows |
| slotmap | 1.1.1 | Zlib | https://github.com/orlp/slotmap | Android, Linux, Windows |
| smallvec | 1.16.6 | MIT OR Apache-2.0 | https://github.com/smallvec/smallvec | Android, Linux, Windows |
| smithay-client-toolkit | 0.19.2 | MIT | https://github.com/smithay/client-toolkit | Linux |
| smithay-client-toolkit | 0.20.0 | MIT | https://github.com/smithay/client-toolkit | Linux |
| smithay-clipboard | 0.7.3 | MIT | https://github.com/smithay/smithay-clipboard | Linux |
| smol_str | 0.2.2 | MIT OR Apache-2.0 | https://github.com/rust-analyzer/smol_str | Linux, Windows |
| smol_str | 0.3.7 | MIT OR Apache-2.0 | https://github.com/rust-lang/rust-analyzer/tree/master/lib/smol_str | Android, Linux, Windows |
| snafu | 0.8.9 | MIT OR Apache-2.0 | https://github.com/shepmaster/snafu | Android, Linux, Windows |
| snafu-derive | 0.8.9 | MIT OR Apache-2.0 | https://github.com/shepmaster/snafu | Android, Linux, Windows |
| softbuffer | 0.4.8 | MIT OR Apache-2.0 | https://github.com/rust-windowing/softbuffer | Linux, Windows |
| spin_on | 0.1.1 | Apache-2.0 OR MIT |  | Android, Linux, Windows |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 | https://github.com/storyyeller/stable_deref_trait | Android, Linux, Windows |
| static_assertions | 1.1.0 | MIT OR Apache-2.0 | https://github.com/nvzqz/static-assertions-rs | Linux, Windows |
| strength_reduce | 0.2.4 | MIT OR Apache-2.0 | http://github.com/ejmahler/strength_reduce | Android, Linux, Windows |
| strict-num | 0.1.1 | MIT | https://github.com/RazrFalcon/strict-num | Android, Linux, Windows |
| strum | 0.28.0 | MIT | https://github.com/Peternator7/strum | Android, Linux, Windows |
| strum_macros | 0.28.0 | MIT | https://github.com/Peternator7/strum | Android, Linux, Windows |
| subtle | 2.6.1 | BSD-3-Clause | https://github.com/dalek-cryptography/subtle | Android, Linux |
| svgtypes | 0.16.2 | Apache-2.0 OR MIT | https://github.com/linebender/svgtypes | Android, Linux, Windows |
| swash | 0.2.10 | Apache-2.0 OR MIT | https://github.com/dfrg/swash | Android, Linux, Windows |
| symphonia | 0.6.1 | MPL-2.0 | https://github.com/pdeljanov/Symphonia | Android, Linux, Windows |
| symphonia-bundle-flac | 0.6.1 | MPL-2.0 | https://github.com/pdeljanov/Symphonia | Android, Linux, Windows |
| symphonia-bundle-mp3 | 0.6.1 | MPL-2.0 | https://github.com/pdeljanov/Symphonia | Android, Linux, Windows |
| symphonia-codec-aac | 0.6.1 | MPL-2.0 | https://github.com/pdeljanov/Symphonia | Android, Linux, Windows |
| symphonia-codec-alac | 0.6.1 | MPL-2.0 | https://github.com/pdeljanov/Symphonia | Android, Linux, Windows |
| symphonia-codec-pcm | 0.6.1 | MPL-2.0 | https://github.com/pdeljanov/Symphonia | Android, Linux, Windows |
| symphonia-codec-vorbis | 0.6.1 | MPL-2.0 | https://github.com/pdeljanov/Symphonia | Android, Linux, Windows |
| symphonia-common | 0.6.1 | MPL-2.0 | https://github.com/pdeljanov/Symphonia | Android, Linux, Windows |
| symphonia-core | 0.6.1 | MPL-2.0 | https://github.com/pdeljanov/Symphonia | Android, Linux, Windows |
| symphonia-format-isomp4 | 0.6.1 | MPL-2.0 | https://github.com/pdeljanov/Symphonia | Android, Linux, Windows |
| symphonia-format-mkv | 0.6.1 | MPL-2.0 | https://github.com/pdeljanov/Symphonia | Android, Linux, Windows |
| symphonia-format-ogg | 0.6.1 | MPL-2.0 | https://github.com/pdeljanov/Symphonia | Android, Linux, Windows |
| symphonia-format-riff | 0.6.1 | MPL-2.0 | https://github.com/pdeljanov/Symphonia | Android, Linux, Windows |
| symphonia-metadata | 0.6.1 | MPL-2.0 | https://github.com/pdeljanov/Symphonia | Android, Linux, Windows |
| syn | 2.0.119 | MIT OR Apache-2.0 | https://github.com/dtolnay/syn | Android, Linux, Windows |
| syn | 3.0.10 | MIT OR Apache-2.0 | https://github.com/dtolnay/syn | Android, Linux, Windows |
| synstructure | 0.14.0 | MIT | https://github.com/mystor/synstructure | Android, Linux, Windows |
| sys-locale | 0.3.2 | MIT OR Apache-2.0 | https://github.com/1Password/sys-locale | Android, Linux, Windows |
| taffy | 0.15.1 | MIT | https://github.com/DioxusLabs/taffy | Android, Linux, Windows |
| text-size | 1.1.1 | MIT OR Apache-2.0 | https://github.com/rust-analyzer/text-size | Android, Linux, Windows |
| thiserror | 1.0.69 | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror | Android, Linux |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror | Android, Linux, Windows |
| thiserror-impl | 1.0.69 | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror | Android, Linux |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror | Android, Linux, Windows |
| tiny-skia | 0.11.4 | BSD-3-Clause | https://github.com/RazrFalcon/tiny-skia | Linux |
| tiny-skia | 0.12.0 | BSD-3-Clause | https://github.com/linebender/tiny-skia | Android, Linux, Windows |
| tiny-skia-path | 0.11.4 | BSD-3-Clause | https://github.com/RazrFalcon/tiny-skia/tree/master/path | Linux |
| tiny-skia-path | 0.12.0 | BSD-3-Clause | https://github.com/linebender/tiny-skia/tree/master/path | Android, Linux, Windows |
| tinystr | 0.8.5 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| tinyvec | 1.13.3 | Zlib OR Apache-2.0 OR MIT | https://github.com/Lokathor/tinyvec | Android, Linux, Windows |
| tiny-xlib | 0.2.5 | MIT OR Apache-2.0 OR Zlib | https://github.com/rust-windowing/tiny-xlib | Linux |
| toml_datetime | 1.1.3+spec-1.1.0 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml | Android, Linux, Windows |
| toml_edit | 0.25.19+spec-1.1.0 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml | Android, Linux, Windows |
| toml_parser | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml | Android, Linux, Windows |
| toml_writer | 1.2.0+spec-1.1.0 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml | Android, Linux, Windows |
| tracing | 0.1.44 | MIT | https://github.com/tokio-rs/tracing | Linux, Windows |
| tracing-attributes | 0.1.31 | MIT | https://github.com/tokio-rs/tracing | Linux |
| tracing-core | 0.1.36 | MIT | https://github.com/tokio-rs/tracing | Linux, Windows |
| transpose | 0.2.3 | MIT OR Apache-2.0 | https://github.com/ejmahler/transpose | Android, Linux, Windows |
| ttf-parser | 0.25.1 | MIT OR Apache-2.0 | https://github.com/harfbuzz/ttf-parser | Linux |
| typed-index-collections | 3.5.0 | MIT OR Apache-2.0 | https://github.com/zheland/typed-index-collections | Android, Linux, Windows |
| unicase | 2.10.0 | MIT OR Apache-2.0 | https://github.com/seanmonstar/unicase | Android, Linux, Windows |
| unicode-bidi | 0.3.18 | MIT OR Apache-2.0 | https://github.com/servo/unicode-bidi | Android, Linux, Windows |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 | https://github.com/dtolnay/unicode-ident | Android, Linux, Windows |
| unicode-linebreak | 0.1.5 | Apache-2.0 | https://github.com/axelf4/unicode-linebreak | Android, Linux, Windows |
| unicode-script | 0.5.9 | MIT OR Apache-2.0 | https://github.com/unicode-rs/unicode-script | Android, Linux, Windows |
| unicode-segmentation | 1.13.3 | MIT OR Apache-2.0 | https://github.com/unicode-rs/unicode-segmentation | Android, Linux, Windows |
| unicode-vo | 0.1.0 | MIT/Apache-2.0 | https://github.com/RazrFalcon/unicode-vo | Android, Linux, Windows |
| unicode-width | 0.2.3 | MIT OR Apache-2.0 | https://github.com/unicode-rs/unicode-width | Android, Linux, Windows |
| unicode-xid | 0.2.6 | MIT OR Apache-2.0 | https://github.com/unicode-rs/unicode-xid | Android, Linux, Windows |
| untrusted | 0.9.0 | ISC | https://github.com/briansmith/untrusted | Android, Linux |
| ureq | 3.5.2 | MIT OR Apache-2.0 | https://github.com/algesten/ureq | Android, Linux, Windows |
| ureq-proto | 0.7.1 | MIT OR Apache-2.0 | https://github.com/algesten/ureq-proto | Android, Linux, Windows |
| url | 2.5.8 | MIT OR Apache-2.0 | https://github.com/servo/rust-url | Android, Linux, Windows |
| usvg | 0.49.0 | Apache-2.0 OR MIT | https://github.com/linebender/resvg | Android, Linux, Windows |
| utf_types | 1.0.0 | Apache-2.0 OR MIT | https://github.com/hsivonen/utf_types | Android, Linux, Windows |
| utf8_iter | 1.1.0 | Apache-2.0 OR MIT | https://github.com/hsivonen/utf8_iter | Android, Linux, Windows |
| utf8-zero | 0.8.1 | MIT OR Apache-2.0 | https://github.com/algesten/utf8-zero | Android, Linux, Windows |
| uuid | 1.29.3 | Apache-2.0 OR MIT | https://github.com/uuid-rs/uuid | Linux, Windows |
| visibility | 0.1.1 | Zlib OR MIT OR Apache-2.0 | https://github.com/danielhenrymantilla/visibility.rs | Android, Linux, Windows |
| vtable | 0.5.1 | MIT OR Apache-2.0 | https://github.com/slint-ui/slint | Android, Linux, Windows |
| vtable-macro | 0.5.1 | MIT OR Apache-2.0 | https://github.com/slint-ui/slint | Android, Linux, Windows |
| wayland-backend | 0.3.17 | MIT | https://github.com/smithay/wayland-rs | Linux |
| wayland-client | 0.31.15 | MIT | https://github.com/smithay/wayland-rs | Linux |
| wayland-csd-frame | 0.3.0 | MIT | https://github.com/rust-windowing/wayland-csd-frame | Linux |
| wayland-cursor | 0.31.14 | MIT | https://github.com/smithay/wayland-rs | Linux |
| wayland-protocols | 0.32.13 | MIT | https://github.com/smithay/wayland-rs | Linux |
| wayland-protocols-experimental | 20250721.0.1 | MIT | https://github.com/smithay/wayland-rs | Linux |
| wayland-protocols-misc | 0.3.12 | MIT | https://github.com/smithay/wayland-rs | Linux |
| wayland-protocols-plasma | 0.3.12 | MIT | https://github.com/smithay/wayland-rs | Linux |
| wayland-protocols-wlr | 0.3.12 | MIT | https://github.com/smithay/wayland-rs | Linux |
| wayland-scanner | 0.31.11 | MIT | https://github.com/smithay/wayland-rs | Linux |
| wayland-sys | 0.31.11 | MIT | https://github.com/smithay/wayland-rs | Linux |
| webbrowser | 1.2.7 | MIT OR Apache-2.0 | https://github.com/amodm/webbrowser-rs | Linux, Windows |
| webpki-roots | 1.0.12 | CDLA-Permissive-2.0 | https://github.com/rustls/webpki-roots | Android, Linux |
| weezl | 0.1.12 | MIT OR Apache-2.0 | https://github.com/image-rs/weezl | Android, Linux, Windows |
| windowfunctions | 0.1.1 | MIT | https://github.com/HEnquist/windowfunctions-rs | Android, Linux, Windows |
| windows | 0.62.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | Android, Linux, Windows |
| windows_x86_64_msvc | 0.52.6 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | Windows |
| windows-collections | 0.3.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | Android, Linux, Windows |
| windows-core | 0.62.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | Android, Linux, Windows |
| windows-future | 0.3.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | Android, Linux, Windows |
| windows-implement | 0.60.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | Android, Linux, Windows |
| windows-interface | 0.59.3 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | Android, Linux, Windows |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | Android, Linux, Windows |
| windows-numerics | 0.3.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | Android, Linux, Windows |
| windows-result | 0.4.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | Android, Linux, Windows |
| windows-strings | 0.5.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | Android, Linux, Windows |
| windows-sys | 0.52.0 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | Windows |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | Windows |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | Windows |
| windows-threading | 0.2.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | Android, Linux, Windows |
| winit | 0.30.13 | Apache-2.0 | https://github.com/rust-windowing/winit | Linux, Windows |
| winnow | 1.0.5 | MIT | https://github.com/winnow-rs/winnow | Android, Linux, Windows |
| writeable | 0.6.4 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| x11-dl | 2.21.0 | MIT | https://github.com/AltF02/x11-rs.git | Linux |
| x11rb | 0.13.2 | MIT OR Apache-2.0 | https://github.com/psychon/x11rb | Linux |
| x11rb-protocol | 0.13.2 | MIT OR Apache-2.0 | https://github.com/psychon/x11rb | Linux |
| xcursor | 0.3.11 | MIT | https://github.com/esposm03/xcursor-rs | Linux |
| xkbcommon-dl | 0.4.2 | MIT | https://github.com/rust-windowing/xkbcommon-dl | Linux |
| xkeysym | 0.2.1 | MIT OR Apache-2.0 OR Zlib | https://github.com/notgull/xkeysym | Linux |
| xmlwriter | 0.1.0 | MIT | https://github.com/RazrFalcon/xmlwriter | Android, Linux, Windows |
| yazi | 0.2.2 | Apache-2.0 OR MIT | https://github.com/dfrg/yazi | Android, Linux, Windows |
| yeslogic-fontconfig-sys | 6.0.1 | MIT | https://github.com/yeslogic/fontconfig-rs | Linux |
| yoke | 0.8.4 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| yoke-derive | 0.8.4 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| zbus | 5.19.0 | MIT | https://github.com/z-galaxy/zbus/ | Linux |
| zbus_macros | 5.19.0 | MIT | https://github.com/z-galaxy/zbus/ | Linux |
| zbus_names | 4.3.5 | MIT | https://github.com/z-galaxy/zbus/ | Linux |
| zbus_xml | 5.2.1 | MIT | https://github.com/z-galaxy/zbus/ | Linux |
| zbus-lockstep | 0.5.2 | MIT | https://github.com/luukvanderduim/zbus-lockstep | Linux |
| zbus-lockstep-macros | 0.5.2 | MIT | https://github.com/luukvanderduim/zbus-lockstep | Linux |
| zcheapstr | 1.4.0 | MIT | https://github.com/z-galaxy/zcheapstr/ | Linux |
| zeno | 0.3.3 | Apache-2.0 OR MIT | https://github.com/dfrg/zeno | Android, Linux, Windows |
| zerocopy | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | https://github.com/google/zerocopy | Linux |
| zerofrom | 0.1.8 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| zerofrom-derive | 0.1.8 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| zeroize | 1.9.2 | Apache-2.0 OR MIT | https://github.com/RustCrypto/utils | Android, Linux, Windows |
| zerotrie | 0.2.5 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| zerovec | 0.11.9 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| zerovec-derive | 0.11.7 | Unicode-3.0 | https://github.com/unicode-org/icu4x | Android, Linux, Windows |
| zmij | 1.0.23 | MIT | https://github.com/dtolnay/zmij | Android, Linux, Windows |
| zune-core | 0.5.3 | MIT OR Apache-2.0 OR Zlib | https://github.com/etemesi254/zune-image | Android, Linux, Windows |
| zune-jpeg | 0.5.15 | MIT OR Apache-2.0 OR Zlib | https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpeg | Android, Linux, Windows |
| zvariant | 5.15.1 | MIT | https://github.com/z-galaxy/zbus/ | Linux |
| zvariant_derive | 5.15.1 | MIT | https://github.com/z-galaxy/zbus/ | Linux |
| zvariant_utils | 4.2.0 | MIT | https://github.com/z-galaxy/zbus/ | Linux |
