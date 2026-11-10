cmake_minimum_required(VERSION 3.24)
project(coyotl_spike_embedded LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

# Spike finding: QQuickFramebufferObject's renderer returns a QOpenGLFramebufferObject,
# which lives in Qt6::OpenGL (LGPLv3). SPEC section 3's module allowlist must add it.
find_package(Qt6 6.8 REQUIRED COMPONENTS Quick OpenGL)
qt_standard_project_setup(REQUIRES 6.8)

set(LIBMPV_PREFIX "${CMAKE_CURRENT_SOURCE_DIR}/../libmpv-lgpl/out" CACHE PATH "LGPL libmpv bundle")
set(STREAM_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../smb-stream")
set(STREAM_LIB "${STREAM_DIR}/target/release/libhuehuecoyotl_spike_stream.a")

# The product uses Corrosion; for the spike a plain cargo call is enough.
add_custom_command(OUTPUT ${STREAM_LIB}
  COMMAND cargo build --release --lib
  WORKING_DIRECTORY ${STREAM_DIR}
  DEPENDS ${STREAM_DIR}/src/lib.rs ${STREAM_DIR}/Cargo.toml
  COMMENT "cargo build (coyotl stream callbacks)")
add_custom_target(rust_stream DEPENDS ${STREAM_LIB})

qt_add_executable(coyotl-spike-embedded
  main.cpp mpvplayer.h mpvplayer.cpp mpvitem.h mpvitem.cpp coyotl_stream.h)
qt_add_qml_module(coyotl-spike-embedded URI Spike VERSION 1.0 QML_FILES Main.qml)
add_dependencies(coyotl-spike-embedded rust_stream)

target_include_directories(coyotl-spike-embedded PRIVATE "${LIBMPV_PREFIX}/include")
target_link_libraries(coyotl-spike-embedded PRIVATE
  Qt6::Quick Qt6::OpenGL
  "${LIBMPV_PREFIX}/lib/libmpv.so"
  ${STREAM_LIB} pthread dl m)
set_target_properties(coyotl-spike-embedded PROPERTIES
  BUILD_RPATH "${LIBMPV_PREFIX}/lib")
