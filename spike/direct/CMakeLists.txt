cmake_minimum_required(VERSION 3.24)
project(coyotl_spike_direct LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt6 6.8 REQUIRED COMPONENTS Quick)
qt_standard_project_setup(REQUIRES 6.8)

set(EMB "${CMAKE_CURRENT_SOURCE_DIR}/../embedded")
set(LIBMPV_PREFIX "${CMAKE_CURRENT_SOURCE_DIR}/../libmpv-lgpl/out" CACHE PATH "libmpv prefix (include/ and lib/)")
set(STREAM_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../smb-stream")
if(WIN32)
  set(STREAM_LIB "${STREAM_DIR}/target/release/huehuecoyotl_spike_stream.lib")
  set(MPV_LIB "${LIBMPV_PREFIX}/lib/mpv.lib")      # import library of libmpv-2.dll
  set(SYS_LIBS ws2_32 userenv ntdll bcrypt advapi32 secur32 crypt32 ncrypt)
else()
  set(STREAM_LIB "${STREAM_DIR}/target/release/libhuehuecoyotl_spike_stream.a")
  set(MPV_LIB "${LIBMPV_PREFIX}/lib/libmpv.so")
  set(SYS_LIBS pthread dl m)
endif()

add_custom_command(OUTPUT ${STREAM_LIB}
  COMMAND cargo build --release --lib
  WORKING_DIRECTORY ${STREAM_DIR}
  DEPENDS ${STREAM_DIR}/src/lib.rs ${STREAM_DIR}/Cargo.toml)
add_custom_target(rust_stream_direct DEPENDS ${STREAM_LIB})

qt_add_executable(coyotl-spike-direct WIN32
  main.cpp ${EMB}/mpvplayer.h ${EMB}/mpvplayer.cpp ${EMB}/coyotl_stream.h)
qt_add_qml_module(coyotl-spike-direct URI SpikeDirect VERSION 1.0 QML_FILES Overlay.qml)
add_dependencies(coyotl-spike-direct rust_stream_direct)
target_include_directories(coyotl-spike-direct PRIVATE "${LIBMPV_PREFIX}/include" ${EMB})
target_link_libraries(coyotl-spike-direct PRIVATE Qt6::Quick ${MPV_LIB} ${STREAM_LIB} ${SYS_LIBS})
set_target_properties(coyotl-spike-direct PROPERTIES BUILD_RPATH "${LIBMPV_PREFIX}/lib")
