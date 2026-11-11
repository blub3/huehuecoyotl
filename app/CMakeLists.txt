qt_add_executable(coyotl WIN32 MACOSX_BUNDLE
  src/main.cpp
  src/corebridge.h src/corebridge.cpp src/coyotl/event_sink.h
  src/mpvplayer.h src/mpvplayer.cpp
  src/mpvvideoitem.h src/mpvvideoitem.cpp
  src/playbackcontroller.h src/playbackcontroller.cpp
  src/models.h src/models.cpp
  src/desktopintegration.h src/desktopintegration.cpp
  src/benchmark.h src/benchmark.cpp)

set_source_files_properties(qml/Theme.qml qml/Nav.qml qml/Appearance.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)
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
    qml/Toast.qml
    qml/AppLogo.qml
    qml/Appearance.qml
    qml/Nav.qml
    qml/NavRail.qml
    qml/Shell.qml
    qml/PosterCard.qml
    qml/GridPage.qml
    qml/MoviesPage.qml
    qml/ShowsPage.qml
    qml/DetailPage.qml
    qml/FixMatchDialog.qml
    qml/SearchPage.qml
    qml/SourcesPage.qml
    qml/NetworkSharePage.qml
    qml/SettingsPage.qml
    qml/ReviewPage.qml
    qml/NoticesPage.qml)

# Third-party notices (SPEC 10, step 6), shown under Settings → About and installed as a file.
# Release builds must have cargo-about; developer builds without it embed a short stub.
# Packaging builds without cargo-about (Flatpak) pass a file generated beforehand.
set(COYOTL_NOTICES_FILE "" CACHE FILEPATH "Pre-generated THIRD_PARTY_NOTICES.txt (from tools/notices.sh)")
find_program(COYOTL_CARGO_ABOUT cargo-about)
set(_notices ${CMAKE_CURRENT_BINARY_DIR}/notices/THIRD_PARTY_NOTICES.txt)
if(COYOTL_NOTICES_FILE)
  configure_file(${COYOTL_NOTICES_FILE} ${_notices} COPYONLY)
elseif(COYOTL_CARGO_ABOUT AND NOT COYOTL_SYSTEM_MPV AND NOT WIN32)
  add_custom_command(OUTPUT ${_notices}
    COMMAND ${CMAKE_COMMAND} -E make_directory ${CMAKE_CURRENT_BINARY_DIR}/notices
    COMMAND ${CMAKE_SOURCE_DIR}/tools/notices.sh ${_notices} ${COYOTL_LIBMPV_PREFIX}
    DEPENDS ${CMAKE_SOURCE_DIR}/tools/notices.sh ${CMAKE_SOURCE_DIR}/tools/about.hbs
            ${CMAKE_SOURCE_DIR}/about.toml ${CMAKE_SOURCE_DIR}/Cargo.lock ${CMAKE_SOURCE_DIR}/NOTICE
            ${COYOTL_LIBMPV_PREFIX}/BUILDINFO.txt
    COMMENT "Generating third-party notices"
    VERBATIM)
else()
  message(STATUS "cargo-about not found: the app embeds a stub instead of the full third-party notices")
  file(READ ${CMAKE_SOURCE_DIR}/NOTICE _notice)
  file(WRITE ${_notices} "${_notice}\nThis development build does not include the full licence texts. "
                         "Release builds generate them with tools/notices.sh.\n")
endif()
qt_add_resources(coyotl notices PREFIX /notices BASE ${CMAKE_CURRENT_BINARY_DIR}/notices FILES ${_notices})
install(FILES ${_notices} DESTINATION ${CMAKE_INSTALL_DATADIR}/doc/coyotl)

# Window icon (the taskbar and title bar on Windows and X11; Wayland uses the desktop entry) and
# the logo in the UI (AppLogo.qml, from the 512 px size).
qt_add_resources(coyotl icons PREFIX /icons BASE ${CMAKE_SOURCE_DIR}/packaging/icons/hicolor FILES
  ${CMAKE_SOURCE_DIR}/packaging/icons/hicolor/32x32/apps/io.github.blub3.Huehuecoyotl.png
  ${CMAKE_SOURCE_DIR}/packaging/icons/hicolor/64x64/apps/io.github.blub3.Huehuecoyotl.png
  ${CMAKE_SOURCE_DIR}/packaging/icons/hicolor/256x256/apps/io.github.blub3.Huehuecoyotl.png
  ${CMAKE_SOURCE_DIR}/packaging/icons/hicolor/512x512/apps/io.github.blub3.Huehuecoyotl.png)
if(WIN32)
  # The .exe icon for Explorer and shortcuts.
  target_sources(coyotl PRIVATE src/coyotl.rc)
endif()

qt_add_translations(coyotl TS_FILE_DIR ${CMAKE_SOURCE_DIR}/app/i18n TS_FILE_BASE coyotl RESOURCE_PREFIX /i18n
                    LUPDATE_OPTIONS -no-obsolete)

target_include_directories(coyotl PRIVATE src)
target_compile_definitions(coyotl PRIVATE COYOTL_VERSION="${PROJECT_VERSION}")
target_link_libraries(coyotl PRIVATE
  Qt6::Core Qt6::Gui Qt6::OpenGL Qt6::Qml Qt6::Quick Qt6::QuickControls2
  coyotl_bridge libmpv)
if(UNIX AND NOT APPLE)
  target_sources(coyotl PRIVATE src/mpris.h src/mpris.cpp)
  target_link_libraries(coyotl PRIVATE Qt6::DBus)
endif()
if(NOT COYOTL_SYSTEM_MPV)
  set_target_properties(coyotl PROPERTIES BUILD_RPATH "${COYOTL_LIBMPV_PREFIX}/lib" INSTALL_RPATH "$ORIGIN/../lib")
endif()
coyotl_check_qt_modules(coyotl)

install(TARGETS coyotl BUNDLE DESTINATION . RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR})
if(WIN32 AND NOT COYOTL_SYSTEM_MPV)
  # The libmpv bundle's DLLs live next to coyotl.exe: copied into the build tree so it runs
  # from there, and installed with it. windeployqt adds Qt's own DLLs at packaging time.
  file(GLOB _mpv_dlls "${COYOTL_LIBMPV_PREFIX}/bin/*.dll")
  add_custom_command(TARGET coyotl POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E copy_if_different ${_mpv_dlls} $<TARGET_FILE_DIR:coyotl>
    VERBATIM)
  install(FILES ${_mpv_dlls} DESTINATION ${CMAKE_INSTALL_BINDIR})
endif()
if(UNIX AND NOT APPLE)
  set(_appid io.github.blub3.Huehuecoyotl)
  set(_pkg ${CMAKE_SOURCE_DIR}/packaging/linux)
  install(FILES ${_pkg}/${_appid}.desktop DESTINATION ${CMAKE_INSTALL_DATADIR}/applications)
  install(FILES ${_pkg}/${_appid}.metainfo.xml DESTINATION ${CMAKE_INSTALL_DATADIR}/metainfo)
  install(DIRECTORY ${CMAKE_SOURCE_DIR}/packaging/icons/hicolor DESTINATION ${CMAKE_INSTALL_DATADIR}/icons)
  # The LGPL libmpv bundle goes next to the binary's rpath ($ORIGIN/../lib), unless it is
  # already installed into the same prefix (Flatpak builds it as a module into /app).
  get_filename_component(_mpv_prefix "${COYOTL_LIBMPV_PREFIX}" REALPATH)
  get_filename_component(_install_prefix "${CMAKE_INSTALL_PREFIX}" REALPATH)
  if(NOT COYOTL_SYSTEM_MPV AND NOT _mpv_prefix STREQUAL _install_prefix)
    install(DIRECTORY ${COYOTL_LIBMPV_PREFIX}/lib/ DESTINATION lib
            FILES_MATCHING PATTERN "*.so.*" PATTERN "pkgconfig" EXCLUDE)
  endif()
endif()
