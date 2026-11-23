// Issue #48: the show page's episode list asks the core for pages only. Scrolling a season of 200
// episodes issues paged requests and never one per tile, and download progress updates one tile
// in place instead of rebuilding the list. The core is replaced by a stub backend that answers
// requests asynchronously, like the real one.
#include <QQmlContext>
#include <QQuickItem>
#include <QQuickView>
#include <QSignalSpy>
#include <QTimer>
#include <QtTest>

#include "episodemodel.h"

namespace {
struct Call {
    qint64 season;
    int offset, limit;
};

QVariantMap episode(int n)
{
    return {{"id", qlonglong(1000 + n)},
            {"kind", "episode"},
            {"title", QStringLiteral("Episode %1").arg(n)},
            {"episodeNo", n},
            {"onDisk", true},
            {"files", QVariantList{QVariantMap{{"id", qlonglong(5000 + n)}, {"online", true}}}},
            {"save", QVariantMap{{"state", "none"}, {"saveable", 1}}}};
}
} // namespace

class TestEpisodeModel : public QObject {
    Q_OBJECT
    QList<Call> calls;
    QList<EpisodeModel *> models;
    int total = 200;
    quint64 lastId = 0;

    void installStub()
    {
        EpisodeModel::setBackend({[this](qint64 season, int offset, int limit) {
                                      const quint64 id = ++lastId;
                                      calls.append({season, offset, limit});
                                      QTimer::singleShot(5, this, [this, id, offset, limit] {
                                          QVariantList rows;
                                          for (int n = offset + 1; n <= qMin(total, offset + limit); ++n)
                                              rows.append(episode(n));
                                          for (auto *m : std::as_const(models))
                                              m->onPage(id, total, offset, rows);
                                      });
                                      return id;
                                  },
                                  [](quint64) {}, [this](EpisodeModel *m) { models.append(m); }});
    }

  private slots:
    void init()
    {
        calls.clear();
        models.clear();
        total = 200;
        installStub();
    }

    void scrollingASeasonAsksForPagesOnly()
    {
        QQuickView view;
        EpisodeModel model;
        view.rootContext()->setContextProperty("episodes", &model);
        view.setSource(QUrl("data:text/plain,"
                            "import QtQuick\n"
                            "ListView { width: 800; height: 600; model: episodes; cacheBuffer: 200;\n"
                            "  delegate: Rectangle { width: 800; height: 112; color: 'gray' } }"));
        QCOMPARE(view.status(), QQuickView::Ready);
        view.show();
        QVERIFY(QTest::qWaitForWindowExposed(&view));
        auto *list = view.rootObject();

        model.setSeason(51);
        QTRY_COMPARE(model.count(), EpisodeModel::kPage);
        QTest::qWait(50);
        // One screen of tiles needs the first page only.
        QCOMPARE(calls.size(), 1);

        // Scroll to the end, a screen at a time.
        for (int i = 0; i < 100 && model.count() < total; ++i) {
            list->setProperty("contentY", list->property("contentY").toDouble() + 600);
            QTest::qWait(20);
        }
        QTRY_COMPARE(model.count(), total);
        list->setProperty("contentY", list->property("contentHeight").toDouble());
        QTest::qWait(50);

        // Every request was the next page of this season, in order; none asked for one tile.
        const int pages = (total + EpisodeModel::kPage - 1) / EpisodeModel::kPage;
        QCOMPARE(calls.size(), pages);
        for (int i = 0; i < calls.size(); ++i) {
            QCOMPARE(calls[i].season, 51);
            QCOMPARE(calls[i].offset, i * EpisodeModel::kPage);
            QCOMPARE(calls[i].limit, EpisodeModel::kPage);
        }
        QCOMPARE(model.get(199).value("episodeNo").toInt(), 200);
    }

    void progressUpdatesOneTileInPlace()
    {
        total = 20;
        EpisodeModel model;
        model.setSeason(51);
        QTRY_COMPARE(model.count(), 20);
        QSignalSpy reset(&model, &QAbstractItemModel::modelReset);
        QSignalSpy changed(&model, &QAbstractItemModel::dataChanged);
        QSignalSpy inserted(&model, &QAbstractItemModel::rowsInserted);

        // Ten episodes save at once; each running main file moves its own tile.
        for (int n = 1; n <= 10; ++n)
            model.onProgress({{"id", qlonglong(n)},
                              {"mainId", qlonglong(n)},
                              {"itemId", qlonglong(1000 + n)},
                              {"kind", "saved"},
                              {"state", "running"},
                              {"bytesDone", 50.0 * n},
                              {"size", 1000.0}});
        QCOMPARE(changed.size(), 10);
        for (int i = 0; i < changed.size(); ++i) {
            const auto from = changed[i][0].value<QModelIndex>(), to = changed[i][1].value<QModelIndex>();
            QCOMPARE(from.row(), i);
            QCOMPARE(to.row(), i);
        }
        const QVariantMap save = model.get(3).value("save").toMap();
        QCOMPARE(save.value("state").toString(), QStringLiteral("running"));
        QCOMPARE(save.value("bytesDone").toDouble(), 200.0);

        // Exports, subtitles (not the main file) and unknown episodes leave the tiles alone.
        model.onProgress(
            {{"id", 1}, {"mainId", 1}, {"itemId", 1001}, {"kind", "export"}, {"state", "running"}});
        model.onProgress(
            {{"id", 7}, {"mainId", 1}, {"itemId", 1001}, {"kind", "saved"}, {"state", "running"}});
        model.onProgress({{"id", 9}, {"mainId", 9}, {"itemId", 99}, {"kind", "saved"}, {"state", "running"}});
        QCOMPARE(changed.size(), 10);

        // A change of state re-reads the loaded rows in one request, in place.
        calls.clear();
        model.onItemChanged(1004);
        model.onItemChanged(1005);
        QTRY_COMPARE(calls.size(), 1);
        QCOMPARE(calls[0].offset, 0);
        QTRY_VERIFY(!model.loading());
        QCOMPARE(reset.size(), 0);
        QCOMPARE(inserted.size(), 0);
    }
};

QTEST_MAIN(TestEpisodeModel)
#include "tst_episodemodel.moc"
