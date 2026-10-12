# dgop-sysmon-cosmic-applet

A COSMIC panel system monitor applet (CPU, memory, processes) that reads its
metrics from the [`dgop`](https://github.com/AvengeMedia/dgop) command-line tool.
The applet runs `dgop` every few seconds and draws the result in the panel and
in its popup.

## Requirements

- `dgop` on your `PATH` (the applet calls it as `dgop`).
- Rust toolchain, `just`, `pkg-config`, `clang`/`libclang` (for bindgen).

## Install

    just install            # → ~/.local/bin and ~/.local/share/applications
    sudo just prefix=/usr install

Then add "System Monitor" in COSMIC Settings → Panel → Applets.

> The desktop ID (`com.system76.CosmicAppletSysmon`) is the same as the stock
> COSMIC system monitor applet, so this build replaces it.

Upstream applet: [pop-os/cosmic-applets](https://github.com/pop-os/cosmic-applets)
(GPL-3.0-only). See `LICENSE`.
