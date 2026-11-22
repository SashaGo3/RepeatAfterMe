# Architecture

One Rust library (`core/`) does all the work. The window (`app/`, Tauri) and the screen (`ui/`, Svelte) show the state of
the library. They also send your key presses and clicks to the library.

## Components

| Part | How it works |
|---|---|
| Audio files | `symphonia` reads mp3, m4a (AAC, ALAC), wav, flac, and ogg. |
| Word times | `whisper.cpp` *small.en* (190 MB, quantized) transcribes each lesson one time. The text of the PDF is a hint for the model. The word times come from cross-attention (DTW). The app trims each word time to the actual speech. |
| Words ↔ PDF | First, the app aligns the spoken words with the printed words in order. Then the layout of the page corrects the result. See [Alignment rules](#alignment-rules). |
| Phrases | A printed sentence is one phrase, also when the teacher pauses in it. Grid cells and items between `·` are separate phrases. |
| PDF | `pdfium` gives the words with their boxes, and the page images. One engine does both, so the highlights align exactly with the text. |
| Speakers and microphone | `cpal` (CoreAudio, WASAPI, ALSA, PulseAudio, PipeWire). Your turn stops after 0.8 s of silence. The app cuts the silence before and after your take. |

### Alignment rules

- The app reads a grid by column. A word goes on the row of its column, also when the model hears it as a different word of the grid.
- Cells that the teacher says together stay together ("that one", "the hour").
- The app joins split words ("all together" → "altogether").
- The app accepts words that sound the same.
- A misheard word between two matches goes between these matches.
- A stray match moves back next to its neighbors.

## Source map

| Path | Content |
|---|---|
| `core/src/align.rs` | Transcript ↔ PDF, phrases |
| `core/src/transcribe.rs` | whisper.cpp |
| `core/src/session.rs` | The practice loop (tested on a fake clock) |
| `core/src/engine.rs` | Lessons, devices, progress, events |
| `core/src/library.rs`, `progress.rs` | The lesson list, the pairs of files, the progress file |
| `core/src/pdf.rs`, `audio.rs`, `device.rs` | PDF, audio math, speakers and microphone |
| `app/` | Tauri: commands, menu, `ram://` page images, bundles |
| `ui/src/lib/keys.ts` | All keyboard shortcuts. The key handler, the labels, the bar, and the help screen read this file. |

## Engine tools

These tools in `core/examples/` test the engine on real lessons.

| Command | What it does |
|---|---|
| `align_report <data dir> <out dir>` | Writes a report for each lesson in a data folder: printed words that nobody heard, heard words without a place, matches that have no support, and each spoken word → printed word. |
| `align_check <pdf> <transcript.json>` | Shows the phrases that a transcript gives. `MAP=150-300` shows where each word between 150 s and 300 s went. |
| `transcribe_check <audio> <pdf> [reference.json]` | Transcribes a lesson. Then it compares the word times and the phrases with a reference. |
| `engine_check <data dir> <course folder> <lesson>` | Operates the full engine on the real speakers (listen only, no microphone). |

To measure a change to `align.rs`:

1. Run `align_report` before the change.
2. Run `align_report` after the change, into a different folder.
3. Compare the two folders with `diff -r`.

To see the effect of the layout rules, set `RAM_ALIGN_BASELINE=1`. This turns the layout rules off.

## UI without the engine

You can work on the UI in a normal browser. `npm --prefix ui run dev` uses a mock backend (`ui/src/lib/mock.ts`, dev
builds only). The mock plays a sample lesson from `ui/dev/`.

## Releases

GitHub Actions builds the installers from this source code when you push a `v*` tag
([`release.yml`](../.github/workflows/release.yml)).

- The macOS app gets a Developer ID signature, and Apple notarizes it. `app/tauri.release.conf.json` turns on the hardened runtime for this build.
- Local builds have an ad hoc signature.
- `scripts/build-app.sh` changes the source paths that compilers keep for error messages: the repository folder becomes `.` and your home folder becomes `~`. Then the script makes sure that the result does not contain your home folder path.
