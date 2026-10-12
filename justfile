# dgop-sysmon-cosmic-applet: COSMIC system monitor applet fed by the dgop CLI.
# Installs to ~/.local by default. System-wide: `sudo just prefix=/usr install`.

prefix := env_var_or_default('PREFIX', env_var('HOME') + '/.local')
bindir := prefix / 'bin'
appdir := prefix / 'share/applications'

default: build

build:
    cargo build --release --workspace

install: build
    install -Dm0755 target/release/cosmic-applet-sysmon {{bindir}}/cosmic-applet-sysmon
    install -Dm0644 applets/sysmon/data/com.system76.CosmicAppletSysmon.desktop {{appdir}}/com.system76.CosmicAppletSysmon.desktop

uninstall:
    rm -f {{bindir}}/cosmic-applet-sysmon {{appdir}}/com.system76.CosmicAppletSysmon.desktop

test:
    cargo test --workspace

clean:
    cargo clean
