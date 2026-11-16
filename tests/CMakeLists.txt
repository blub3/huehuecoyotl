# Qt Quick Test: focus and key navigation of the player controls (driven with a stub player) and
# of the show page's episode tiles (with stub episodes)
# (SPEC 10, CI step 4). The QML under test is copied into a small module `CoyotlUi` so the
# tests do not need the app's C++ types.
set(_ui_dir "${CMAKE_CURRENT_BINARY_DIR}/imports/CoyotlUi")
set(_ui_files Theme.qml FocusButton.qml TrackMenu.qml PlayerControls.qml
  # Issue #48: the episode tile and the components it shares with the detail page.
  MediaInfo.js EpisodeTile.qml VersionPicker.qml TechSummary.qml TechDetails.qml SaveMenu.qml
  # Issue #52: the TMDB key status in Settings.
  TmdbKeyStatus.qml)
file(MAKE_DIRECTORY "${_ui_dir}")
set(_qmldir "module CoyotlUi\nsingleton Theme 1.0 Theme.qml\n")
foreach(f IN LISTS _ui_files)
  configure_file("${CMAKE_SOURCE_DIR}/app/qml/${f}" "${_ui_dir}/${f}" COPYONLY)
  if(NOT f STREQUAL "Theme.qml" AND f MATCHES "\\.qml$")
    get_filename_component(_name "${f}" NAME_WE)
    string(APPEND _qmldir "${_name} 1.0 ${f}\n")
  endif()
endforeach()
file(WRITE "${_ui_dir}/qmldir" "${_qmldir}")

qt_add_executable(tst_qml tst_qml.cpp)
target_link_libraries(tst_qml PRIVATE Qt6::QuickTest Qt6::Quick Qt6::QuickControls2)
target_compile_definitions(tst_qml PRIVATE
  QUICK_TEST_SOURCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}/qml"
  COYOTL_TEST_IMPORTS="${CMAKE_CURRENT_BINARY_DIR}/imports")
coyotl_check_qt_modules(tst_qml)
add_test(NAME qml COMMAND tst_qml -platform offscreen)

# Issue #48: the show page's episode model pages through a season and updates tiles in place,
# driven by a stub backend instead of the core.
qt_add_executable(tst_episodemodel tst_episodemodel.cpp ${CMAKE_SOURCE_DIR}/app/src/episodemodel.cpp)
target_include_directories(tst_episodemodel PRIVATE ${CMAKE_SOURCE_DIR}/app/src)
target_link_libraries(tst_episodemodel PRIVATE Qt6::Test Qt6::Quick Qt6::Qml)
coyotl_check_qt_modules(tst_episodemodel)
add_test(NAME episodemodel COMMAND tst_episodemodel -platform offscreen)

# Issue #8: gapless playback and one fixture per music format, through the bundled libmpv with
# the music player's options, rendered into mpv's file output (no audio device needed).
qt_add_executable(tst_musicplayback tst_musicplayback.cpp)
target_include_directories(tst_musicplayback PRIVATE ${CMAKE_SOURCE_DIR}/app/src)
target_link_libraries(tst_musicplayback PRIVATE Qt6::Test libmpv)
coyotl_check_qt_modules(tst_musicplayback)
add_test(NAME musicplayback COMMAND tst_musicplayback)

# Issue #66: audiobook playback: resume inside a later file, one timeline across files, speed and
# the sleep timer, through the bundled libmpv with the book player's options (null output) and a
# stub instead of the core.
qt_add_executable(tst_bookplayback tst_bookplayback.cpp
  ${CMAKE_SOURCE_DIR}/app/src/bookplayer.h ${CMAKE_SOURCE_DIR}/app/src/bookplayer.cpp)
target_include_directories(tst_bookplayback PRIVATE ${CMAKE_SOURCE_DIR}/app/src)
target_link_libraries(tst_bookplayback PRIVATE Qt6::Test Qt6::Qml libmpv)
coyotl_check_qt_modules(tst_bookplayback)
add_test(NAME bookplayback COMMAND tst_bookplayback)
