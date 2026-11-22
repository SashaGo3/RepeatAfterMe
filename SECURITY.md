# Security

Repeat After Me works offline: there is no server to attack and no account to take over. The parts
worth a second look are what it reads from files you give it: zip files of lessons
(`core/src/engine.rs`, `unzip`), PDFs (pdfium, `core/src/pdf.rs`), audio files (symphonia,
`core/src/audio.rs`), progress backups (`restore_progress`), and the `ram://` page protocol in
`app/src/main.rs`.

If you find something — a crafted zip that writes outside the app's folder, a file that crashes the
app, a backup that does something it should not — please open a private security advisory on GitHub
rather than a public issue. Include the input that triggers it. Expect a reply within a week.
