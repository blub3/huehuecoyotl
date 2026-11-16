# Hi-Res audio: manual checklist (issue #9)

The automated test (`tests/tst_musicplayback.cpp`, `bitPerfect`) proves that in bit-perfect
mode the app hands mpv's output exactly the decoded samples: a 24-bit, 96 kHz FLAC comes out
byte for byte as FFmpeg's reference decode. It cannot show what reaches the hardware. This
checklist does, with a USB DAC that displays the sample rate it receives.

Run it for every release that touches music playback, once outside the Flatpak (direct ALSA
device) and once in the Flatpak (PipeWire).

## What you need

- A USB DAC with a sample-rate display (or one that shows the rate in its own app).
- Test files, made with FFmpeg (no copyrighted music needed), in a music folder of their own:

  ```bash
  mkdir -p ~/Music/hires-test/Hi-Res/Rates && cd ~/Music/hires-test/Hi-Res/Rates
  n=1
  for r in 44100 48000 88200 96000 192000; do
    ffmpeg -loglevel error -f lavfi -i "sine=frequency=1000:sample_rate=$r:duration=30" \
      -ac 2 -c:a flac -sample_fmt s32 -bits_per_raw_sample 24 \
      -metadata title="Tone $r" -metadata artist="Hi-Res" -metadata album="Rates" \
      -metadata track=$n "0$n - $r.flac"
    n=$((n+1))
  done
  ```

- The folder added under **Sources** as **Music**. Keep the volume of the DAC or amplifier low:
  in bit-perfect mode the app's volume is fixed at 100 %.

## A. Outside the Flatpak: direct ALSA device

1. Settings → Audio → **Music output**: choose the DAC's entry marked "direct" (its
   "Front output" or "IEC958" device). Turn **Bit-perfect music** on.
2. Stop other programs that play sound (a browser tab, the system sounds): a direct device
   cannot be shared.
3. Play the album *Rates* from the first track and open **Now Playing**.

For each track (44.1, 48, 88.2, 96, 192 kHz):

- [ ] The DAC shows the track's rate.
- [ ] Now Playing shows the **Bit-perfect** badge and "Output: … · ALSA · exclusive" with the
      track's rate.
- [ ] No note about converting.

Rate changes:

- [ ] Going from 44.1 to 48 kHz and from 96 to 192 kHz by itself (the next track), the DAC
      follows; you may hear a short gap there (the device is reopened), never a click inside a
      track.
- [ ] Previous, Next and jumping in the queue between rates: the DAC follows each time.

A device that refuses:

- [ ] Play a rate the DAC does not support (if there is one): playback goes on, Now Playing
      shows no **Bit-perfect** badge and says the device does not take that rate and what it
      was converted to. Never silence.
- [ ] Start another program that holds the DAC (for example `speaker-test -D hw:CARD=<DAC>`),
      then play: the app plays on the system's default device instead and says so in red
      under the output line. Never silence.

Back to normal:

- [ ] Turn **Bit-perfect music** off: the volume buttons on Now Playing work again, the
      ReplayGain choice applies again, and the output line no longer says "exclusive".

## B. In the Flatpak: PipeWire

The Flatpak has no permission for direct device access (SPEC section 12), so the list offers
no "direct" devices. PipeWire switches the DAC's rate only when the rate is allowed in its
configuration and no other program plays.

1. Allow the rates (see the user guide, *Bit-perfect music*), restart PipeWire.
2. Settings → Audio → Music output: the DAC (or the system default); **Bit-perfect music** on.
3. Play the album *Rates* with no other program playing.

- [ ] For each track the DAC shows the track's rate (`pw-top` shows the same).
- [ ] Now Playing never shows the **Bit-perfect** badge here: it shows "Output: … · PipeWire"
      and that the system mixer may resample.
- [ ] With another program playing at 48 kHz, the DAC stays at 48 kHz and the app's output
      line says so; playback goes on.

## Result

Note the date, the app version, the DAC, the distribution and anything that did not match, and
add it to the release's pull request.
