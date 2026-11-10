#include <QQmlEngine>
#include <QQuickStyle>
#include <QtQuickTest>

class Setup : public QObject {
    Q_OBJECT
  public slots:
    void applicationAvailable() { QQuickStyle::setStyle("Basic"); }
    void qmlEngineAvailable(QQmlEngine *engine)
    {
        engine->addImportPath(QStringLiteral(COYOTL_TEST_IMPORTS));
    }
};

QUICK_TEST_MAIN_WITH_SETUP(coyotl, Setup)
#include "tst_qml.moc"
