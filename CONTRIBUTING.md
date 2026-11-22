# Contributing to Repeat After Me

Thanks for looking. Repeat After Me is a small, focused app and a one-person project most of the time,
so the best contributions are the ones that keep it that way: focused, tested, and in the spirit of the
design rules below.

## Before you start

- **Bugs and small fixes:** open an issue or a pull request straight away.
- **Features:** open an issue first and describe the problem you are solving, not only the feature.
  The app deliberately says no to a lot (accounts, servers, scoring your pronunciation), and it is
  kinder to find that out before writing code.
- **Course material:** never attach a course's recordings or PDFs to an issue or a pull request; they
  belong to whoever made the course. Describe the lesson instead, or make a small sample of your own
  like the one in `samples/`.

## Setup

Needs Rust, Node 20+, CMake (for whisper.cpp) and, on Linux, `libwebkit2gtk-4.1-dev libasound2-dev`.

```sh
scripts/fetch-vendor.sh                          # speech model + pdfium for this OS → vendor/ (once)
npm --prefix ui ci
cd app && ../ui/node_modules/.bin/tauri dev      # run
```

`npm --prefix ui run dev` runs the screen alone in a browser with a mock backend (`ui/src/lib/mock.ts`),
the quickest loop for UI work. Try a build without touching your own progress:
`RAM_DATA_DIR=/tmp/ram-test <the app's executable>`.

## Design rules worth knowing

- **Offline, always.** Nothing talks to a server; lessons, progress and your voice stay on the computer.
- **You decide.** Nothing grades you: the app plays the teacher and you back to back, and you say good
  or again.
- **One engine, a thin window.** All logic is in the Rust library (`core/`); the window (`app/`) and the
  screen (`ui/`) only show its state and pass on what you press.
- **Every shortcut in one table.** `ui/src/lib/keys.ts`: the handler, the button labels and the help
  sheet all read it.
- **Words ↔ PDF follows the page.** `core/src/align.rs` places spoken words by the shape of the page
  (columns, cells, reading order), never by special cases for particular words. A change there is
  checked on real lessons: run `align_report` before and after and compare (see the README).

## Checks before a pull request

```sh
RAM_PDFIUM_DIR=vendor/pdfium/<your OS>/lib cargo test -p ram-core   # core
npm --prefix ui run check                                          # TypeScript + Svelte
```

If you touched a screen, walk it in light and dark mode and in a narrow window. If you touched
`align.rs`, add a test for the case you fixed and include the `align_report` difference.

## Commit style

Short imperative subject, a body that says *why*. One change per pull request.
