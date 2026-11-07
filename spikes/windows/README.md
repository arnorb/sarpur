# Windows spikes (for the Windows user)

These checks run on a real Windows 10 or 11 PC. They tell us whether sarpur can use ffmpeg on Windows the way we plan to.

## Spike 5: ffmpeg on Windows

1. Download `spike5.ps1` from this folder.
2. Open the Start menu, type **PowerShell**, and open **Windows PowerShell**.
3. Go to the folder where you saved the file, for example: `cd $HOME\Downloads`
4. Run it: `powershell -ExecutionPolicy Bypass -File spike5.ps1`
5. It takes a few minutes. It downloads ffmpeg (about 195 MB) and one short RÚV weather bulletin.
6. Send back the file **`sarpur-spike5-results.json`** from your Desktop.
   If Windows shows a warning (SmartScreen or Defender), take a screenshot and send that too. Writing down what you clicked helps.

Nothing is installed. Everything goes in `%TEMP%\sarpur-spike5`, which you can delete afterwards.

What it checks:

- The pinned ffmpeg build downloads, matches its SHA-256, and Defender lets it run.
- ffmpeg can write a file whose full path is longer than 260 characters and contains Icelandic letters. The `.part` file is then renamed to its final name.
- ffmpeg reports progress through `-progress pipe:1`.
- Sending `q` stops ffmpeg within 15 seconds and leaves a playable file.
- Closing a Job Object kills ffmpeg. That's how sarpur will make sure no `ffmpeg.exe` is left running when it quits.

The RÚV episode the script uses expires after a while. If the download step fails with a 404, ask for an updated script.

## Spike 8, Windows half

This one needs the first sarpur build, which doesn't exist yet. Instructions come with that build.
