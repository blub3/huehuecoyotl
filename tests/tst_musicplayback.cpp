// Issue #8: music playback through the bundled libmpv with the music player's options
// (app/src/musicoptions.h), rendered into mpv's file output (ao=pcm) instead of a device.
//  - gapless: one continuous 441 Hz sine split into two FLAC files at a sample boundary plays
//    back as one tone, sample for sample: no gap, no extra samples, no discontinuity;
//  - formats: one generated fixture per format of SPEC section 2 decodes to about a second of
//    sound.
//  - bit-perfect (issue #9): a 24-bit, 96 kHz FLAC played in bit-perfect mode comes out
//    byte for byte as FFmpeg's reference decode (with the volume at 50 % it does not, so the
//    comparison can tell); and the rules for "exclusive" and "Bit-perfect" on Now Playing.
// Fixtures are generated with ffmpeg in a temporary folder; without ffmpeg the tests skip.
#include <QElapsedTimer>
#include <QFile>
#include <QProcess>
#include <QTemporaryDir>
#include <QTest>
#include <QtEndian>
#include <clocale>
#include <cmath>
#include <mpv/client.h>

#include "audiopath.h"
#include "musicoptions.h"

namespace {
constexpr int kRate = 44100;

bool haveFfmpeg()
{
    QProcess p;
    p.start("ffmpeg", {"-version"});
    return p.waitForFinished(10000) && p.exitCode() == 0;
}

bool ffmpeg(const QStringList &args)
{
    QProcess p;
    p.setProcessChannelMode(QProcess::MergedChannels);
    p.start("ffmpeg", QStringList{"-loglevel", "error", "-y"} + args);
    if (!p.waitForFinished(60000) || p.exitCode() != 0) {
        qWarning() << "ffmpeg failed:" << args << p.readAll();
        return false;
    }
    return true;
}

// Play the files back to back into `out` as raw signed 16-bit mono at 44.1 kHz, or (`native`)
// in bit-perfect mode as they are, at `volume`.
bool render(const QStringList &files, const QString &out, bool native = false, int volume = 100)
{
    std::setlocale(LC_NUMERIC, "C");
    mpv_handle *mpv = mpv_create();
    if (!mpv) return false;
    if (setMusicOptions(mpv) != 0) qWarning() << "music options rejected";
    const QByteArray file = out.toUtf8();
    mpv_set_option_string(mpv, "ao", "pcm");
    mpv_set_option_string(mpv, "ao-pcm-file", file.constData());
    mpv_set_option_string(mpv, "ao-pcm-waveheader", "no");
    if (!native) {
        mpv_set_option_string(mpv, "audio-format", "s16");
        mpv_set_option_string(mpv, "audio-channels", "mono");
        mpv_set_option_string(mpv, "audio-samplerate", "44100");
    }
    mpv_set_option_string(mpv, "terminal", "no");
    if (mpv_initialize(mpv) < 0) {
        mpv_terminate_destroy(mpv);
        return false;
    }
    if (native) {
        setBitPerfect(mpv, volume == 100, "no");
        const QByteArray v = QByteArray::number(volume);
        mpv_set_property_string(mpv, "volume", v.constData());
    }
    bool first = true;
    for (const QString &f : files) {
        const QByteArray u = f.toUtf8();
        const char *args[] = {"loadfile", u.constData(), first ? "replace" : "append", nullptr};
        mpv_command(mpv, args);
        first = false;
    }
    // Played through when mpv is idle again after the last file.
    bool started = false, done = false, failed = false;
    QElapsedTimer t;
    t.start();
    while (!done && t.elapsed() < 60000) {
        mpv_event *e = mpv_wait_event(mpv, 1.0);
        if (e->event_id == MPV_EVENT_START_FILE) started = true;
        if (e->event_id == MPV_EVENT_END_FILE &&
            static_cast<mpv_event_end_file *>(e->data)->reason == MPV_END_FILE_REASON_ERROR)
            failed = true;
        if (e->event_id == MPV_EVENT_IDLE && started) done = true;
    }
    mpv_terminate_destroy(mpv); // flushes and closes the output file
    return done && !failed;
}

QVector<qint16> samples(const QString &path)
{
    QFile f(path);
    if (!f.open(QIODevice::ReadOnly)) return {};
    const QByteArray b = f.readAll();
    QVector<qint16> s(b.size() / 2);
    for (int i = 0; i < s.size(); ++i)
        s[i] = qFromLittleEndian<qint16>(b.constData() + 2 * i);
    return s;
}
} // namespace

class TestMusicPlayback : public QObject {
    Q_OBJECT
    QTemporaryDir m_dir;

  private slots:
    void initTestCase()
    {
        if (!haveFfmpeg()) QSKIP("ffmpeg is not installed");
        QVERIFY(m_dir.isValid());
    }

    void gapless()
    {
        // 441 Hz at 44.1 kHz: exactly 100 samples per period. Two seconds, cut at sample 44100.
        const QString a = m_dir.filePath("a.flac"), b = m_dir.filePath("b.flac");
        const QString tone = QStringLiteral("sine=frequency=441:sample_rate=%1:duration=2").arg(kRate);
        QVERIFY(ffmpeg({"-f", "lavfi", "-i", tone, "-ac", "1", "-af", "atrim=end_sample=44100", "-c:a",
                        "flac", "-sample_fmt", "s16", a}));
        QVERIFY(ffmpeg({"-f", "lavfi", "-i", tone, "-ac", "1", "-af",
                        "atrim=start_sample=44100,asetpts=PTS-STARTPTS", "-c:a", "flac", "-sample_fmt", "s16",
                        b}));
        const QString out = m_dir.filePath("gapless.raw");
        QVERIFY(render({a, b}, out));
        const QVector<qint16> s = samples(out);
        // No gap: exactly the samples of both files (mpv may add a few at the very end).
        QVERIFY2(s.size() >= 2 * kRate && s.size() < 2 * kRate + 2048,
                 qPrintable(QStringLiteral("%1 samples").arg(s.size())));
        // No discontinuity: every sample is where the one tone puts it.
        double amp = 0;
        for (int i = 0; i < 2 * kRate; ++i)
            amp = std::max(amp, double(std::abs(s[i])));
        QVERIFY(amp > 1000);
        int worst = 0, at = -1;
        for (int i = 0; i < 2 * kRate; ++i) {
            const double ideal = amp * std::sin(2 * M_PI * 441.0 * i / kRate);
            const int err = int(std::abs(s[i] - ideal));
            if (err > worst) {
                worst = err;
                at = i;
            }
        }
        QVERIFY2(worst < amp * 0.02, qPrintable(QStringLiteral("off by %1 at sample %2").arg(worst).arg(at)));
    }

    void bitPerfect()
    {
        // A 997 Hz tone on one channel, noise on the other, at 24 bit and 96 kHz.
        const QString in = m_dir.filePath("hires.flac"), ref = m_dir.filePath("hires-ref.raw");
        QVERIFY(ffmpeg({"-f", "lavfi", "-i", "sine=frequency=997:sample_rate=96000:duration=1", "-f", "lavfi",
                        "-i", "anoisesrc=r=96000:a=0.05:d=1", "-filter_complex", "[0][1]amerge=inputs=2",
                        "-c:a", "flac", "-sample_fmt", "s32", "-bits_per_raw_sample", "24", in}));
        QVERIFY(ffmpeg({"-i", in, "-f", "s32le", "-c:a", "pcm_s32le", ref}));
        QFile r(ref);
        QVERIFY(r.open(QIODevice::ReadOnly));
        const QByteArray expected = r.readAll();
        QCOMPARE(expected.size(), 96000 * 2 * 4);

        const QString out = m_dir.filePath("hires-out.raw");
        QVERIFY(render({in}, out, true));
        QFile o(out);
        QVERIFY(o.open(QIODevice::ReadOnly));
        const QByteArray got = o.readAll();
        QCOMPARE(got.size(), expected.size());
        QVERIFY2(got == expected, "bit-perfect output differs from the reference decode");

        // The comparison can tell: at 50 % volume the samples differ.
        const QString quieter = m_dir.filePath("hires-50.raw");
        QVERIFY(render({in}, quieter, true, 50));
        QFile q(quieter);
        QVERIFY(q.open(QIODevice::ReadOnly));
        QVERIFY(q.readAll() != expected);
    }

    void signalPathRules()
    {
        const QVariantMap in{{"samplerate", 96000}, {"format", "s32p"}, {"channel-count", 2}};
        const QVariantMap same{{"samplerate", 96000}, {"format", "s32"}, {"channel-count", 2}};
        const QVariantMap resampled{{"samplerate", 48000}, {"format", "s32"}, {"channel-count", 2}};
        const QVariantMap s16{{"samplerate", 96000}, {"format", "s16"}, {"channel-count", 2}};
        // Exclusive only on hardware devices with exclusive access requested; never shared.
        QVERIFY(audiopath::exclusive(true, "alsa", "alsa/hw:1,0"));
        QVERIFY(audiopath::exclusive(true, "alsa", "alsa/front:CARD=DAC,DEV=0"));
        QVERIFY(audiopath::exclusive(true, "alsa", "alsa/iec958:CARD=DAC,DEV=0"));
        QVERIFY(!audiopath::exclusive(true, "alsa", "alsa"));
        QVERIFY(!audiopath::exclusive(true, "alsa", "alsa/sysdefault:CARD=DAC"));
        QVERIFY(!audiopath::exclusive(true, "alsa", "alsa/dmix:CARD=DAC,DEV=0"));
        QVERIFY(!audiopath::exclusive(true, "alsa", "alsa/plughw:CARD=DAC,DEV=0"));
        QVERIFY(!audiopath::exclusive(true, "pipewire", "pipewire/alsa_output.usb"));
        QVERIFY(!audiopath::exclusive(true, "pulse", "auto"));
        QVERIFY(!audiopath::exclusive(false, "alsa", "alsa/hw:1,0"));
        QVERIFY(audiopath::exclusive(true, "wasapi", "wasapi/{id}"));
        // Bit-perfect only exclusive, unfiltered and unchanged.
        QVERIFY(audiopath::bitPerfect(in, same, true, false));
        QVERIFY(!audiopath::bitPerfect(in, same, false, false)); // shared: never
        QVERIFY(!audiopath::bitPerfect(in, same, true, true));   // a filter
        QVERIFY(!audiopath::bitPerfect(in, resampled, true, false));
        QVERIFY(!audiopath::bitPerfect(in, s16, true, false));
        QVERIFY(!audiopath::bitPerfect({}, same, true, false));
        // The devices offered: default, PipeWire's outputs (not PulseAudio's next to them),
        // direct ALSA hardware; no plug-ins, no mixing or converting ALSA devices.
        auto dev = [](const char *n) { return QVariantMap{{"name", n}, {"description", n}}; };
        const QVariantList all{dev("auto"),
                               dev("pipewire"),
                               dev("pipewire/usb-dac"),
                               dev("pulse/usb-dac"),
                               dev("alsa"),
                               dev("alsa/speex"),
                               dev("alsa/plughw:CARD=DAC,DEV=0"),
                               dev("alsa/dmix:CARD=DAC,DEV=0"),
                               dev("alsa/front:CARD=DAC,DEV=0")};
        QStringList names;
        for (const QVariant &v : audiopath::offered(all))
            names << v.toMap().value("name").toString() + (v.toMap().value("direct").toBool() ? "*" : "");
        QCOMPARE(names, (QStringList{"auto", "pipewire/usb-dac", "alsa/front:CARD=DAC,DEV=0*"}));
        QCOMPARE(audiopath::offered(all, true).size(), 2); // the Flatpak: no direct hardware
    }

    void formats_data()
    {
        QTest::addColumn<QString>("file");
        QTest::addColumn<QStringList>("codec");
        QTest::newRow("FLAC") << "t.flac" << QStringList{"-c:a", "flac"};
        QTest::newRow("ALAC") << "t-alac.m4a" << QStringList{"-c:a", "alac"};
        QTest::newRow("WAV") << "t.wav" << QStringList{"-c:a", "pcm_s16le"};
        QTest::newRow("AIFF") << "t.aiff" << QStringList{"-c:a", "pcm_s24be"};
        QTest::newRow("MP3") << "t.mp3" << QStringList{"-c:a", "libmp3lame"};
        QTest::newRow("AAC (MP4)") << "t-aac.m4a" << QStringList{"-c:a", "aac"};
        QTest::newRow("AAC (ADTS)") << "t.aac" << QStringList{"-c:a", "aac"};
        QTest::newRow("Ogg Vorbis") << "t.ogg" << QStringList{"-c:a", "libvorbis"};
        QTest::newRow("Opus") << "t.opus" << QStringList{"-ar", "48000", "-c:a", "libopus"};
        QTest::newRow("WavPack") << "t.wv" << QStringList{"-c:a", "wavpack"};
    }

    void formats()
    {
        QFETCH(QString, file);
        QFETCH(QStringList, codec);
        const QString in = m_dir.filePath(file);
        QVERIFY(ffmpeg(QStringList{"-f", "lavfi", "-i", "sine=frequency=441:sample_rate=44100:duration=1",
                                   "-ac", "2", "-metadata", "title=Tone"} +
                       codec + QStringList{in}));
        const QString out = m_dir.filePath(file + ".raw");
        QVERIFY(render({in}, out));
        const QVector<qint16> s = samples(out);
        // About a second (lossy codecs pad a little), and not silence.
        QVERIFY2(s.size() > kRate * 9 / 10 && s.size() < kRate * 13 / 10,
                 qPrintable(QStringLiteral("%1 samples").arg(s.size())));
        double sum = 0;
        for (qint16 v : s)
            sum += double(v) * v;
        QVERIFY(std::sqrt(sum / s.size()) > 500);
    }
};

QTEST_GUILESS_MAIN(TestMusicPlayback)
#include "tst_musicplayback.moc"
