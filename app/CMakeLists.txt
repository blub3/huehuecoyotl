qt_add_executable(coyotl WIN32 MACOSX_BUNDLE
  src/main.cpp
  src/corebridge.h src/corebridge.cpp src/coyotl/event_sink.h
  src/mpvplayer.h src/mpvplayer.cpp
  src/mpvvideoitem.h src/mpvvideoitem.cpp
  src/playbackcontroller.h src/playbackcontroller.cpp
  src/models.h src/models.cpp
  src/desktopintegration.h src/desktopintegration.cpp)

set_source_files_properties(qml/Theme.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)
qt_add_qml_module(coyotl
  URI Coyotl
  VERSION 1.0
  QML_FILES
    qml/Main.qml
    qml/Theme.qml
    qml/HomePage.qml
    qml/BrowserPage.qml
    qml/PlayerPage.qml
    qml/PlayerControls.qml
    qml/TrackMenu.qml
    qml/FocusButton.qml
    qml/PosterTile.qml
    qml/Toast.qml)

qt_add_translations(coyotl TS_FILE_DIR ${CMAKE_SOURCE_DIR}/app/i18n TS_FILE_BASE coyotl RESOURCE_PREFIX /i18n
                    LUPDATE_OPTIONS -no-obsolete)

target_include_directories(coyotl PRIVATE src)
target_compile_definitions(coyotl PRIVATE COYOTL_VERSION="${PROJECT_VERSION}")
target_link_libraries(coyotl PRIVATE
  Qt6::Core Qt6::Gui Qt6::OpenGL Qt6::Qml Qt6::Quick Qt6::QuickControls2
  coyotl_bridge libmpv)
if(UNIX AND NOT APPLE)
  target_link_libraries(coyotl PRIVATE Qt6::DBus)
endif()
if(NOT COYOTL_SYSTEM_MPV)
  set_target_properties(coyotl PROPERTIES BUILD_RPATH "${COYOTL_LIBMPV_PREFIX}/lib" INSTALL_RPATH "$ORIGIN/../lib")
endif()
coyotl_check_qt_modules(coyotl)

install(TARGETS coyotl BUNDLE DESTINATION . RUNTIME DESTINATION bin)
