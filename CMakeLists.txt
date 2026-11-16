cmake_minimum_required(VERSION 3.24)
project(Huehuecoyotl VERSION 2.1.0 DESCRIPTION "Desktop media player" LANGUAGES CXX)
include(GNUInstallDirs)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

option(COYOTL_BUILD_TESTS "Build the Qt tests" ON)
set(COYOTL_LIBMPV_PREFIX "${CMAKE_CURRENT_SOURCE_DIR}/third_party/mpv/out" CACHE PATH
    "Prefix of the LGPL libmpv bundle (include/ and lib/), built by third_party/mpv")
option(COYOTL_SYSTEM_MPV "Link the system libmpv via pkg-config instead (distribution packages; may be GPL)" OFF)

# ---- Qt (SPEC 3: LGPL modules only; enforced by cmake/QtModuleAllowlist.cmake) --------------
set(COYOTL_QT_COMPONENTS Core Gui OpenGL Qml Quick QuickControls2 LinguistTools)
if(UNIX AND NOT APPLE)
  list(APPEND COYOTL_QT_COMPONENTS DBus)
endif()
if(COYOTL_BUILD_TESTS)
  list(APPEND COYOTL_QT_COMPONENTS QuickTest Test)
endif()
find_package(Qt6 6.8 REQUIRED COMPONENTS ${COYOTL_QT_COMPONENTS})
qt_standard_project_setup(REQUIRES 6.8 I18N_SOURCE_LANGUAGE en I18N_TRANSLATED_LANGUAGES de)
qt_policy(SET QTP0004 NEW)
include(cmake/QtModuleAllowlist.cmake)

# ---- Rust core through Corrosion and cxx (SPEC 8, 10) ----------------------------------------
include(FetchContent)
FetchContent_Declare(Corrosion
  GIT_REPOSITORY https://github.com/corrosion-rs/corrosion.git
  GIT_TAG v0.6.1)
FetchContent_MakeAvailable(Corrosion)
corrosion_import_crate(MANIFEST_PATH ${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml
                       CRATES huehuecoyotl-core-api)
corrosion_add_cxxbridge(coyotl_bridge CRATE huehuecoyotl_core_api FILES lib.rs)
target_include_directories(coyotl_bridge PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/app/src)
if(WIN32)
  target_link_libraries(coyotl_bridge PUBLIC ws2_32 userenv ntdll bcrypt advapi32)
endif()

# ---- libmpv (SPEC 3: own LGPL build, shared, never static) -----------------------------------
add_library(libmpv SHARED IMPORTED GLOBAL)
if(COYOTL_SYSTEM_MPV)
  find_package(PkgConfig REQUIRED)
  pkg_check_modules(MPV REQUIRED IMPORTED_TARGET mpv>=2.2)
  set_target_properties(libmpv PROPERTIES IMPORTED_LOCATION "${MPV_LINK_LIBRARIES}")
  target_link_libraries(libmpv INTERFACE PkgConfig::MPV)
  message(WARNING "Linking the system libmpv. If it is a GPL build, the resulting package is GPL.")
else()
  if(WIN32)
    set(_mpv_lib "${COYOTL_LIBMPV_PREFIX}/bin/libmpv-2.dll")
    set_target_properties(libmpv PROPERTIES IMPORTED_IMPLIB "${COYOTL_LIBMPV_PREFIX}/lib/mpv.lib")
  else()
    set(_mpv_lib "${COYOTL_LIBMPV_PREFIX}/lib/libmpv.so.2")
  endif()
  if(NOT EXISTS "${_mpv_lib}")
    message(FATAL_ERROR "LGPL libmpv not found at ${_mpv_lib}. Build it with third_party/mpv/build.sh "
                        "or pass -DCOYOTL_LIBMPV_PREFIX=... (or -DCOYOTL_SYSTEM_MPV=ON for distribution builds).")
  endif()
  set_target_properties(libmpv PROPERTIES
    IMPORTED_LOCATION "${_mpv_lib}"
    INTERFACE_INCLUDE_DIRECTORIES "${COYOTL_LIBMPV_PREFIX}/include")
endif()

add_subdirectory(app)

if(COYOTL_BUILD_TESTS)
  enable_testing()
  add_subdirectory(tests)
endif()
