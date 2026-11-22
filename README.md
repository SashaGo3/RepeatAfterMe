# Repeat After Me

[![License: MIT](https://img.shields.io/badge/license-MIT-2B2B2E.svg)](LICENSE)
![Platforms](https://img.shields.io/badge/platform-macOS%20%C2%B7%20Windows-2B2B2E.svg)
[![Latest release](https://img.shields.io/github/v/release/SashaGo3/RepeatAfterMe?color=2B2B2E)](https://github.com/SashaGo3/RepeatAfterMe/releases/latest)

Practice pronunciation with the recordings and PDFs of your own course.

![Repeat After Me: a lesson's PDF with the words you got right underlined in green and words to work on marked in orange, next to the practice panel](docs/screenshot.png)

## Features

- The words light up in the PDF while the teacher speaks.
- You practice one phrase at a time: listen, repeat, compare.
- You hear the teacher and then yourself, back to back.
- You decide if your take is good. The app does not grade you.
- The app saves your progress for each lesson.
- Everything runs on your computer. There is no account and no internet connection.
- The app does not save your voice to disk.

## How a practice turn works

1. The teacher says the phrase.
2. You say the phrase. The microphone stops when you stop talking.
3. You hear the teacher, then yourself.
4. If your take is good, press <kbd>Enter</kbd>. The next phrase starts.
5. If you want to try again, press <kbd>R</kbd>.

## Install

Download the installer for your system from the
**[latest release](https://github.com/SashaGo3/RepeatAfterMe/releases/latest)**.

| System | File |
|---|---|
| macOS 11 or later (Apple Silicon and Intel) | `Repeat.After.Me_<version>_universal.dmg` |
| Windows 10 or 11 | `Repeat.After.Me_<version>_x64-setup.exe` |

**macOS**

1. Open the `.dmg` file.
2. Drag **Repeat After Me** to **Applications**.
3. Open the app.

The macOS app has a Developer ID signature and Apple notarized it. macOS opens it without a warning.

**Windows**

1. Open the setup file.
2. If Windows shows "Windows protected your PC", click **More info**, then click **Run anyway**.

The Windows installer does not have a paid code-signing certificate. This is why Windows shows the warning.

**All systems**

The installer is approximately 200 MB. It contains the speech model, so you do not need other downloads.
The first time that you practice, your system asks for access to the microphone. Allow the access.

GitHub Actions builds every installer from this source code
([`release.yml`](.github/workflows/release.yml)).

**Linux:** the code builds on Linux, but nobody tested the Linux app. To try it, [build it from source](#build-from-source).

## Quick start

1. Download the two files in [`samples/`](samples) into one folder. They are a short lesson about the TH sound.
2. Open Repeat After Me.
3. Drag the folder onto the window.
4. Select the lesson in the lesson list.
5. Press <kbd>Space</kbd> to start.

## Add your lessons

A lesson is one recording and one PDF.

To add lessons, drag a folder (or a `.zip` file of a folder) onto the window.
You can also select **File → Add a Folder of Lessons…** (<kbd>⌘/Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>O</kbd>).

The app pairs each recording with a PDF in the same folder. It uses the first match in this list:

1. The PDF with the same name.
2. The PDF with the closest name. For example, "Light L **vs.** Dark L.m4a" pairs with "Light L **and** Dark L.pdf".
3. The only PDF in the folder.

If the pair is wrong, click **⋯ → Use a different PDF…** in the lesson list.

The app does not move your files. It unpacks a `.zip` file into its data folder.
The app links your progress to the content of a lesson. If you move or rename the files, you keep your progress.

The first time that you open a lesson, the app listens to the recording to find each word.
This takes approximately 1 second for each 20 seconds of audio. After that, the lesson opens immediately.

## Keyboard shortcuts

Each button shows its key. Press <kbd>?</kbd> to see all keys.

| Key | Action |
|---|---|
| <kbd>Space</kbd> | Start, skip to my turn, stop my turn, or hear both again |
| <kbd>Enter</kbd> | Good: go to the next phrase |
| <kbd>R</kbd> | Again |
| <kbd>B</kbd> / <kbd>←</kbd> / <kbd>Esc</kbd> | Go to the previous phrase |
| <kbd>N</kbd> / <kbd>→</kbd> | Go to the next phrase |
| <kbd>T</kbd> / <kbd>M</kbd> | Play the teacher only / play my take only |
| <kbd>F</kbd> / right-click a word | Mark the words of the phrase in orange. Press again to remove the mark. |
| <kbd>O</kbd> | Show all phrases, the phrases to do, or the marked phrases |
| <kbd>L</kbd> | Listen only: play the full lesson with highlights, without turns |
| <kbd>S</kbd> | Stop |
| Click a word | Practice the phrase of this word |
| <kbd>⌘/Ctrl</kbd>+<kbd>L</kbd> | Show the lessons |
| <kbd>⌘/Ctrl</kbd>+<kbd>+</kbd> / <kbd>−</kbd> / <kbd>0</kbd> | Zoom the PDF |
| <kbd>⌘/Ctrl</kbd>+<kbd>R</kbd> | Look for new lessons in your folders |

## Progress and data

**Start a lesson again.** Use one of these:

- **Start over** below the practice buttons.
- **⋯ → Start over…** in the lesson list.
- **File → Start This Lesson Over…**

All phrases go back to "not done". Your marked words and your practice time stay.

**The lesson list** shows your practice time and the date of your last practice for each lesson.
You can search the list. You can sort it by folder, recent practice, most practice, or name.

- **⋯** on a folder: rename the folder, or remove it from the list.
- **⋯** on a lesson: clear the marked words, select a different PDF, or remove the lesson.

When the app starts, it finds new lessons in the folders that you added.
To look again, click **↻** or select **File → Look for New Lessons**. Lessons that you removed stay removed.

**Back up and restore**

- **File → Back Up Progress…** saves all your progress in one file.
- **File → Restore Progress…** adds a backup to your current progress. It does not erase progress.
  You can restore on a different computer, because the app identifies a lesson by its recording.

**Your data.** The data folder holds the lesson list, your progress, and the transcripts.
To open it, select **File → Show Saved Progress**.
In **Settings → Progress**, you can clear all progress (marked words stay) or reset the app.
These actions do not change your lesson files.

## Build from source

You need Rust, Node.js 20 or later, and CMake. On Linux, you also need `libwebkit2gtk-4.1-dev` and `libasound2-dev`.

```sh
scripts/fetch-vendor.sh                          # speech model + pdfium for this OS → vendor/ (once)
npm --prefix ui ci
cd app && ../ui/node_modules/.bin/tauri dev      # run
cd app && ../ui/node_modules/.bin/tauri build    # installer for this OS
cargo test -p ram-core                           # tests (set RAM_PDFIUM_DIR=vendor/pdfium/mac-arm64/lib or similar)
```

`scripts/build-app.sh` builds an installer that does not contain your user name or your folder paths.

To test a build without changes to your own progress, set a different data folder:
`RAM_DATA_DIR=/tmp/ram-test <path to the app executable>`.

For the design of the app, the engine tools, and a map of the source code, read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Contributing

Issues and pull requests are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) before you start.

To report a security problem, read [SECURITY.md](SECURITY.md). Do not open a public issue for it.

## Support

Repeat After Me is free. If it helped your pronunciation, you can [buy me a coffee](https://buymeacoffee.com/sashago3).

## License

[MIT](LICENSE).

The installers also contain:

- The [whisper.cpp](https://github.com/ggml-org/whisper.cpp) *small.en* model (MIT).
- [pdfium](https://pdfium.googlesource.com/pdfium/) (BSD-3-Clause / Apache-2.0).
