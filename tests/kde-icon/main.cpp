// Exercise KDE itself, not a replica of its theme fallback algorithm.
// KF5 5.92: src/kiconloader.cpp findMatchingIcon and src/kicontheme.h.
#include <KIconLoader>
#include <KIconTheme>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QGuiApplication>
#include <QImage>
#include <QPixmap>
#include <QSettings>
#include <QTextStream>
#include <cstdlib>

static void require(bool condition, const QString &message)
{
    if (!condition) {
        QTextStream(stderr) << message << '\n';
        std::exit(1);
    }
}

static void theme(const QString &root, const QString &name)
{
    const auto directory = root + "/icons/" + name;
    require(QDir().mkpath(directory + "/256x256/apps"), "Create theme directory");
    QFile index(directory + "/index.theme");
    require(index.open(QIODevice::WriteOnly), "Create theme index");
    const QByteArray data = "[Icon Theme]\nName=" + name.toUtf8()
        + "\nDirectories=256x256/apps\n"
        + (name == "hicolor" ? QByteArray() : QByteArray("Inherits=hicolor\n"))
        + "[256x256/apps]\nSize=256\nContext=Applications\nType=Fixed\n";
    require(index.write(data) == data.size(), "Write theme index");
}

int main(int argc, char **argv)
{
    QGuiApplication app(argc, argv);
    require(argc == 3, "Expected source icon and source desktop entry");
    const QString id = "io.github.isaiahpettingill.AsepriteMetadataEditor";
    const QString root = qEnvironmentVariable("XDG_DATA_HOME");
    require(!root.isEmpty(), "Test requires isolated XDG_DATA_HOME");
    theme(root, "collision-test");
    theme(root, "hicolor");
    const QString wrongPath = root + "/icons/collision-test/256x256/apps/aseprite.png";
    QImage wrong(256, 256, QImage::Format_ARGB32);
    wrong.fill(Qt::red);
    require(wrong.save(wrongPath), "Write conflicting theme icon");
    const QString ownPath = root + "/icons/hicolor/256x256/apps/" + id + ".png";
    const QImage original(argv[1]);
    require(!original.isNull() && !QImage(ownPath).isNull(), "Installer icon must exist");
    require(QFile::copy(ownPath, root + "/icons/hicolor/256x256/apps/aseprite-metadata-editor.png"),
            "Create historical hicolor icon for collision reproduction");

    KIconTheme::forceThemeForTests("collision-test");
    KIconLoader loader(id);
    auto resolve = [&](const QString &name) { return loader.iconPath(name, -256, true); };
    require(QFileInfo(resolve("aseprite-metadata-editor")).canonicalFilePath()
                == QFileInfo(wrongPath).canonicalFilePath(),
            "Expected historical hyphenated name to collide with aseprite theme icon");
    require(QFileInfo(resolve(id)).canonicalFilePath() == QFileInfo(ownPath).canonicalFilePath(),
            "Unique application icon must resolve to installed hicolor PNG");
    const auto actual = loader.loadIcon(id, KIconLoader::Desktop, 256, KIconLoader::DefaultState,
                                        QStringList(), nullptr, true).toImage();
    require(actual.convertToFormat(QImage::Format_ARGB32)
                == original.convertToFormat(QImage::Format_ARGB32),
            "KDE loaded pixels must equal the supplied icon, not the red theme icon");

    QSettings packaged(argv[2], QSettings::IniFormat);
    packaged.beginGroup("Desktop Entry");
    require(packaged.value("Icon").toString() == id, "Packaged desktop Icon identity mismatch");
    require(packaged.value("StartupWMClass").toString() == id, "Packaged startup identity mismatch");
    QSettings installed(root + "/applications/" + id + ".desktop", QSettings::IniFormat);
    installed.beginGroup("Desktop Entry");
    const auto installedIcon = installed.value("Icon").toString();
    require(installedIcon == ownPath, "Installer desktop must use its absolute icon path");
    require(resolve(installedIcon) == ownPath, "KDE must preserve installer absolute icon path");
    require(installed.value("StartupWMClass").toString() == id, "Installer startup identity mismatch");
    QTextStream(stdout) << "KDE regression passed: historical collision reproduced; new identity and pixels verified.\n";
}
