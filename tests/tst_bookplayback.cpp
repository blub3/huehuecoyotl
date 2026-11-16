// Issue #66: audiobook playback through the bundled libmpv with the book player's options
// (app/src/bookoptions.h), into mpv's null output (no audio device, nothing audible). The core
// is replaced by a stub backend that hands over tickets like the real one.
//  - resume: a book starts exactly at a saved position inside its second file;
//  - one timeline: playback runs from the first file into the second without a stall;
//  - speed: 1.5× plays 1.5 seconds of the book per second, and is remembered for the book;
//  - sleep timer: "end of chapter" fades out and pauses at the chapter's end, and saves there.
// The two files are one continuous tone split in two, generated with ffmpeg; without ffmpeg the
// tests skip.
#include <QElapsedTimer>
#include <QProcess>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>
#include <QTimer>
#include <algorithm>

#include "bookplayer.h"

namespace {
bool ffmpeg(const QStringList &args)
{
    QProcess p;
    p.setProcessChannelMode(QProcess::MergedChannels);
    p.start("ffmpeg", QStringList{"-loglevel", "error", "-y"} + args);
    return p.waitForFinished(60000) && p.exitCode() == 0;
}

// The core's chapter arithmetic, as crates/library/src/books.rs has it (tested there).
qint64 previousChapter(const QList<qint64> &starts, qint64 pos)
{
    const int i = int(std::upper_bound(starts.begin(), starts.end(), pos) - starts.begin()) - 1;
    if (i < 0) return 0;
    if (pos - starts[i] < 3000 && i > 0) return starts[i - 1];
    return starts[i];
}
qint64 nextChapter(const QList<qint64> &starts, qint64 pos)
{
    const auto it = std::upper_bound(starts.begin(), starts.end(), pos);
    return it == starts.end() ? -1 : *it;
}
} // namespace

class TestBookPlayback : public QObject {
    Q_OBJECT
    QTemporaryDir dir;
    QString first, second;
    BookPlayer *player = nullptr;
    QList<QPair<qint64, qint64>> saved;
    QList<double> speeds;
    qint64 ticketAt = 0;

    QVariantMap ticket(qint64 positionMs)
    {
        QVariantList chapters;
        for (int c = 0; c < 4; ++c)
            chapters.append(QVariantMap{{"title", QStringLiteral("Chapter %1").arg(c + 1)},
                                        {"startMs", c * 10000},
                                        {"endMs", (c + 1) * 10000}});
        return {
            {"bookId", 7},
            {"title", "Book"},
            {"authors", "Writer"},
            {"durationMs", 40000},
            {"positionMs", positionMs},
            {"speed", QVariant()},
            {"chapters", chapters},
            {"files", QVariantList{QVariantMap{{"url", first}, {"startMs", 0}, {"durationMs", 20000}},
                                   QVariantMap{{"url", second}, {"startMs", 20000}, {"durationMs", 20000}}}}};
    }

    // Play the book from `positionMs` and wait until it plays.
    bool start(qint64 positionMs)
    {
        ticketAt = positionMs;
        player->playBook(7, positionMs);
        return QTest::qWaitFor(
            [this] { return player->state() == BookPlayer::Playing && player->position() > 0; }, 10000);
    }

  private slots:
    void initTestCase()
    {
        QProcess probe;
        probe.start("ffmpeg", {"-version"});
        if (!probe.waitForFinished(10000) || probe.exitCode() != 0) QSKIP("ffmpeg is not installed");
        first = dir.filePath("01.flac");
        second = dir.filePath("02.flac");
        // One 441 Hz tone, cut at a sample boundary into two 20-second files.
        const QString tone = "sine=frequency=441:sample_rate=44100:duration=40";
        QVERIFY(ffmpeg({"-f", "lavfi", "-i", tone, "-t", "20", "-c:a", "flac", first}));
        QVERIFY(ffmpeg({"-f", "lavfi", "-i", tone, "-ss", "20", "-c:a", "flac", second}));
        // Never a sound card in a test.
        qputenv("COYOTL_BOOK_MPV_OPTS", "ao=null");
        BookPlayer::setBackend({
            [this](qint64, qint64 pos) {
                QTimer::singleShot(10, this, [this, pos] { player->onTicket(1, "bookTicket", ticket(pos)); });
                return quint64(1);
            },
            [this](qint64 book, qint64 pos) { saved.append({book, pos}); },
            [this](qint64, double s) { speeds.append(s); },
            previousChapter,
            nextChapter,
            [](const QList<qint64> &starts, qint64 duration, qint64 pos) {
                const qint64 n = nextChapter(starts, pos);
                return n < 0 ? duration : n;
            },
            {},
        });
        player = new BookPlayer(this);
    }

    void init()
    {
        player->stop();
        saved.clear();
        speeds.clear();
        player->setSpeed(1.0);
        speeds.clear();
    }

    // A position inside the second file: playback starts there, not at the start of the book
    // or of the file.
    void resumesInsideTheSecondFile()
    {
        QVERIFY(start(32345));
        QVERIFY2(qAbs(player->position() - 32.345) < 1.0, qPrintable(QString::number(player->position())));
        QCOMPARE(player->chapter(), 3);
        QCOMPARE(player->chapterTitle(), QStringLiteral("Chapter 4"));
        // Pausing saves the position.
        player->pause();
        QTRY_VERIFY(!player->playing());
        QVERIFY(!saved.isEmpty());
        QCOMPARE(saved.last().first, qint64(7));
        QVERIFY(qAbs(saved.last().second - 32345) < 1500);
    }

    // From the end of the first file into the second: the timeline goes on in real time.
    void playsAcrossFiles()
    {
        QVERIFY(start(18000));
        QTRY_VERIFY_WITH_TIMEOUT(player->position() >= 18.5, 5000);
        QElapsedTimer t;
        t.start();
        QTRY_VERIFY_WITH_TIMEOUT(player->position() >= 21.5, 8000);
        // Three seconds of book in about three seconds: no stall between the files.
        QVERIFY2(t.elapsed() < 3700, qPrintable(QString::number(t.elapsed())));
        QCOMPARE(player->chapter(), 2);
    }

    void speedKeepsTimeAndIsRemembered()
    {
        QVERIFY(start(2000));
        player->setSpeed(1.5);
        QCOMPARE(player->speed(), 1.5);
        QCOMPARE(speeds, QList<double>{1.5});
        QTest::qWait(500);
        const double from = player->position();
        QTest::qWait(2000);
        const double went = player->position() - from;
        QVERIFY2(went > 2.5 && went < 3.6, qPrintable(QString::number(went)));
        // Out of range: clamped to 3.0 and 0.5 in steps of 0.1.
        player->setSpeed(4.0);
        QCOMPARE(player->speed(), 3.0);
        player->setSpeed(0.44);
        QCOMPARE(player->speed(), 0.5);
    }

    // "End of chapter": the chapter [20 s, 30 s) ends at 30 s; the book pauses there after a
    // fade, and the position is saved.
    void sleepsAtTheEndOfTheChapter()
    {
        QVERIFY(start(22000));
        player->sleepAtChapterEnd();
        QCOMPARE(player->sleep(), QStringLiteral("chapter"));
        QTRY_VERIFY_WITH_TIMEOUT(!player->playing(), 15000);
        QVERIFY2(player->position() > 29.0 && player->position() < 30.8,
                 qPrintable(QString::number(player->position())));
        QCOMPARE(player->sleep(), QStringLiteral("off"));
        QVERIFY(!saved.isEmpty());
        QVERIFY(qAbs(saved.last().second - qint64(player->position() * 1000)) < 300);
    }

    void chapterNavigation()
    {
        QVERIFY(start(21000));
        // One second into chapter 3: "previous" goes to chapter 2.
        player->previousChapter();
        QTRY_VERIFY(qAbs(player->position() - 10.0) < 1.0);
        player->nextChapter();
        QTRY_VERIFY(qAbs(player->position() - 20.0) < 1.0);
        QTRY_COMPARE(player->chapter(), 2);
    }

    void cleanupTestCase() { delete player; }
};

QTEST_GUILESS_MAIN(TestBookPlayback)
#include "tst_bookplayback.moc"
