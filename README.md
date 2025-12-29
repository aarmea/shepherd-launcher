# shepherd-launcher

A child-friendly, parent-guided launcher for Wayland, allowing supervised
access to applications and content that you define.

Its primary goal is to return control of child-focused computing to parents,
not software or hardware vendors, by providing:

* the ease-of-use of game consoles
* access to any application that can be run, emulated, or virtualized in desktop Linux
* with granular access controls inspired by and exceeding those in iOS Screen Time

While this repository provides some examples for existing software packages
(including non-free software and abandonware), `shepherd-launcher` is
*non-prescriptive*: as the end user, you are free to use them, not use them,
or write your own.

## Screenshots

### Home screen

TODO: home screen at different times (bedtime vs afternoon) showing different applications

### Time limits

TODO: GIF or video of GCompris a few seconds from closing, emphasizing:
* Countdown clock
* Warning messaging
* Automatic close at end of time
* Icon deliberately missing afterwards -- cooldown

### "access to any application that can be run, emulated, or virtualized"

TODO: show the following running with some subset of the above features highlighted:
* Minecraft
* Steam games (World of Goo, Portal 2)
* ScummVM games (Putt Putt, Secret of Monkey Island)
* Media

Contributions are welcome for improvements and not yet implemented backends,
such as:
* Content-aware media player [TODO: link to issue]
* Pre-booted Steam to improve launch time [TODO: link to issue]
* Android apps via Waydroid, including pre-booting Android if necessary [TODO: link to issue]
* Legacy Win9x via DOSBox, QEMU, or PCem, including scripts to create a boot-to-app image [TODO: link to issue]
* Chrome, including strict sandboxing and support for firewall rules [TODO: link to issue]

## Core concepts

* **Launcher-first**: only one foreground activity at a time
* **Time-scoped execution**: applications are granted time slices, not unlimited sessions
* **Parent-defined policy**: rules live outside the application being run
* **Wrappers, not patches**: existing software is sandboxed, not modified
* **Revocable access**: sessions end predictably and enforceably

## Non-goals

* Modifying or patching third-party applications
* Circumventing DRM or platform protections
* Replacing parental involvement with automation

## Installation

`shepherd-launcher` is pre-alpha and in active development. As such, end-user
binaries and installation instructions are not yet available.

See [CONTRIBUTING.md](./CONTRIBUTING.md) for how to run in development.

Contributions are welcome for:
* a CI step that generates production binaries [TODO: link to issue]
* an installation script [TODO: link to issue]

## Example configuration

For the Minecraft example shown above:

```toml
# Minecraft via mc-installer snap
# Ubuntu: sudo snap install mc-installer
[[entries]]
id = "minecraft"
label = "Minecraft"
icon = "minecraft"

[entries.kind]
type = "snap"
snap_name = "mc-installer"

[entries.availability]
[[entries.availability.windows]]
days = "weekdays"
start = "15:00"
end = "18:00"

[[entries.availability.windows]]
days = "weekends"
start = "10:00"
end = "20:00"

[entries.limits]
max_run_seconds = 1800  # 30 minutes (roughly 3 in-game days)
daily_quota_seconds = 3600  # 1 hour per day
cooldown_seconds = 600  # 10 minute cooldown

[[entries.warnings]]
seconds_before = 600
severity = "info"
message = "10 minutes left - start wrapping up!"

[[entries.warnings]]
seconds_before = 120
severity = "warn"
message = "2 minutes remaining - save your game!"

[[entries.warnings]]
seconds_before = 30
severity = "critical"
message = "30 seconds! Save NOW!"
```

See [config.example.toml](./config.example.toml) for more.

## Development

See [CONTRIBUTING.md](./CONTRIBUTING.md)

## Written in 2025, responsibly

This project stands on the shoulders of giants in systems software and
compatibility infrastructure:

* Wayland and Sway
* Rust
* Snap
* Proton and WINE

This project was written with the assistance of generative AI-based coding
agents. Substantial prompts and design docs provided to agents are disclosed in
[docs/ai](./docs/ai/)
