# shepherd-launcher

A child-friendly, parent-guided launcher for Wayland, allowing supervised
access to applications and content that you define.

Its primary goal is to return control of child-focused computing to parents,
not software or hardware vendors, by providing:

* the ease-of-use of game consoles
* access to any application that can be run, emulated, or virtualized in desktop Linux
* with granular access controls inspired by and exceeding those in iOS Screen Time

While this repository provides some recipes for existing software packages
(including non-free software), `shepherd-launcher` is *non-prescriptive*: as
the end user, you are free to use them, not use them, or write your own.

## Screenshots

TODO:

* home screen at different times showing different applications
* modern proprietary application showcase (Minecraft, individual Steam games)
* emulated application showcase (ScummVM games, 90s edutainment on Win9x, Duolingo via Waydroid)
* externally managed Chrome container for access to school resources
* media showcase (local storage and individual titles from streaming services)
* time limit popup
* "token" system

## Installation

tl;dr:

1. any Linux with Wayland (optional: TPM-based FDE plus BIOS password to prevent tampering)
2. System dependencies (Ubuntu: `apt install curl sway swayidle pkg-config libcairo2-dev libxkbcommon-dev`)
3. Rust (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
4. binaries (TODO: deployable package that depends on Sway and installs the config)
5. test session on login
6. configure auto-login to this session

## Usage

TODO: open lid; play

## Core concepts

* **Launcher-first**: only one foreground activity at a time
* **Time-scoped execution**: applications are granted time slices, not unlimited sessions
* **Parent-defined policy**: rules live outside the application being run
* **Wrappers, not patches**: existing software is sandboxed, not modified
* **Revocable access**: sessions end predictably and enforceably

## Recipes

TODO

* `.desktop` syntax plus rules (time allowance, time-of-day restrictions, network whitelist, etc.)
* wrappers for common applications
* how to write a custom wrapper or custom application

## Development

TODO: `./run-dev`, options

## Contributing

`shepherd-launcher` is licensed under the GPLv3 to preserve end-users' rights.
By submitting a pull request, you agree to license your contributions under the
GPLv3.

Contributions written in part or in whole by generative AI are allowed;
however, they will be reviewed as if you personally authored them.

The authors of `shepherd-launcher` do not condone software or media piracy.
Contributions that explicitly promote or facilitate piracy will be rejected.
Please support developers and creators by obtaining content legally.
