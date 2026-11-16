# SPEC section 3: official builds use only LGPL Qt modules. Linking a module that is GPL-only in
# the open-source edition (Charts, Data Visualization, Virtual Keyboard, ...) would make the
# binary GPL, so any Qt module outside this list fails the build.
#
# The list names what the app links directly. tools/check-qt-runtime.sh additionally checks the
# Qt libraries actually loaded at run time (transitive dependencies included).
set(COYOTL_QT_ALLOWED_MODULES
  Core Gui OpenGL Qml Quick QuickControls2 Svg DBus
  # QML import only (QtQuick.Dialogs: file and folder dialogs, issues #4 and #5); LGPLv3
  QuickDialogs2
  # build-time only / tests
  LinguistTools QuickTest Test)

function(coyotl_check_qt_modules target)
  get_target_property(_libs ${target} LINK_LIBRARIES)
  foreach(_lib IN LISTS _libs)
    if(_lib MATCHES "^Qt6?::([A-Za-z0-9]+)$")
      set(_mod "${CMAKE_MATCH_1}")
      if(NOT _mod IN_LIST COYOTL_QT_ALLOWED_MODULES)
        message(FATAL_ERROR "Qt module '${_mod}' linked by '${target}' is not on the LGPL allowlist "
                            "(cmake/QtModuleAllowlist.cmake). See SPEC section 3.")
      endif()
    endif()
  endforeach()
endfunction()
