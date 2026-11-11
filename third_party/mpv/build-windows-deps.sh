#!/bin/bash
# Cross-build of the LGPL libmpv bundle for Windows x86_64 with MinGW-w64, run inside
# Dockerfile.windows, one dependency per call:  build-windows-deps.sh NAME
# Adapted from mpv's ci/build-mingw64.sh, with GPL parts removed (no --enable-gpl, mpv
# -Dgpl=false, no Lua, JavaScript or curl) and every source pinned.
set -euo pipefail
prefix_dir=/opt/coyotl-mpv-win
TARGET=x86_64-w64-mingw32
mkdir -p "$prefix_dir" /src && cd /src
ln -snf . "$prefix_dir/usr"; ln -snf . "$prefix_dir/local"

export CC=$TARGET-gcc-posix CXX=$TARGET-g++-posix AR=$TARGET-ar NM=$TARGET-nm RANLIB=$TARGET-ranlib
export CFLAGS="-O2 -pipe" LDFLAGS="-fstack-protector-strong"
export PKG_CONFIG_SYSROOT_DIR="$prefix_dir" PKG_CONFIG_LIBDIR="$prefix_dir/lib/pkgconfig"
at_flags="--disable-static --enable-shared"
gitclone() { git clone --depth=1 --recursive --shallow-submodules -b "$2" "$1" "$3"; }

cat >"$prefix_dir/crossfile" <<CF
[built-in options]
buildtype = 'release'
wrap_mode = 'nodownload'
default_library = 'shared'
[binaries]
c = '${CC}'
cpp = '${CXX}'
ar = '${AR}'
nm = '${NM}'
strip = '${TARGET}-strip'
pkg-config = 'pkg-config'
windres = '${TARGET}-windres'
dlltool = '${TARGET}-dlltool'
nasm = 'nasm'
[host_machine]
system = 'windows'
cpu_family = 'x86_64'
cpu = 'x86_64'
endian = 'little'
CF
cmake_args=(-Wno-dev -GNinja -DCMAKE_SYSTEM_PROCESSOR=x86_64 -DCMAKE_SYSTEM_NAME=Windows
            -DCMAKE_FIND_ROOT_PATH="$prefix_dir" -DCMAKE_RC_COMPILER=$TARGET-windres
            -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=ON)

builddir() { rm -rf "$1/builddir"; mkdir -p "$1/builddir"; cd "$1/builddir"; }
install_() {
    if [ -f build.ninja ]; then ninja; DESTDIR="$prefix_dir" ninja install
    else make -j"$(nproc)"; make DESTDIR="$prefix_dir" install; fi
}
gettar() { wget -q "$1" -O "${1##*/}" && tar -xaf "${1##*/}"; }

case "$1" in
iconv)
    gettar https://ftp.gnu.org/pub/gnu/libiconv/libiconv-1.19.tar.gz
    builddir libiconv-1.19; ../configure --host=$TARGET $at_flags; install_ ;;
zlib)
    gettar https://github.com/zlib-ng/zlib-ng/archive/refs/tags/2.3.3.tar.gz
    builddir zlib-ng-2.3.3; cmake .. "${cmake_args[@]}" -DZLIB_COMPAT=ON -DBUILD_TESTING=OFF; install_
    ln -snf libzlib.dll.a "$prefix_dir/lib/libz.dll.a" ;;
shaderc)
    gitclone https://github.com/google/shaderc.git v2026.4 shaderc
    (cd shaderc && ./utils/git-sync-deps)
    builddir shaderc; cmake .. "${cmake_args[@]}" -DBUILD_SHARED_LIBS=OFF -DSHADERC_SKIP_TESTS=ON \
        -DSHADERC_SKIP_EXAMPLES=ON; install_ ;;
spirv-cross)
    gitclone https://github.com/KhronosGroup/SPIRV-Cross vulkan-sdk-1.4.363.0 SPIRV-Cross
    builddir SPIRV-Cross; cmake .. "${cmake_args[@]}" -DSPIRV_CROSS_SHARED=ON -DSPIRV_CROSS_CLI=OFF \
        -DSPIRV_CROSS_STATIC=OFF; install_ ;;
nv-headers)
    gitclone https://github.com/FFmpeg/nv-codec-headers n13.0.19.0 nv-codec-headers
    cd nv-codec-headers; make DESTDIR="$prefix_dir" install ;;
vulkan)
    gitclone https://github.com/KhronosGroup/Vulkan-Headers v1.4.368 Vulkan-Headers
    (builddir Vulkan-Headers; cmake .. "${cmake_args[@]}"; install_)
    gitclone https://github.com/KhronosGroup/Vulkan-Loader v1.4.368 Vulkan-Loader
    builddir Vulkan-Loader; cmake .. "${cmake_args[@]}" -DUSE_GAS=ON; install_ ;;
dav1d)
    gitclone https://code.videolan.org/videolan/dav1d.git 1.5.4 dav1d
    builddir dav1d; meson setup .. --cross-file "$prefix_dir/crossfile" -Denable_tools=false -Denable_tests=false; install_ ;;
lcms2)
    gitclone https://github.com/mm2/Little-CMS.git lcms2.20 lcms2
    builddir lcms2; meson setup .. --cross-file "$prefix_dir/crossfile" -Dtests=disabled -Dutils=false \
        -Dversionedlibs=false; install_ ;;
ffmpeg)
    # LGPL v2.1+: no --enable-gpl, --enable-nonfree or --enable-version3. Decoding only.
    gitclone https://github.com/FFmpeg/FFmpeg.git n8.1.3 ffmpeg
    builddir ffmpeg
    ../configure --pkg-config=pkg-config --target-os=mingw32 --enable-cross-compile \
        --cross-prefix=$TARGET- --arch=x86_64 --cc="$CC" --cxx="$CXX" $at_flags \
        --disable-doc --disable-programs --disable-encoders --disable-muxers \
        --disable-indevs --disable-outdevs --enable-muxer=spdif \
        --enable-libdav1d --enable-d3d11va --enable-dxva2 --enable-ffnvcodec --enable-nvdec \
        --enable-cuvid --enable-vulkan --enable-network --enable-zlib
    install_
    grep -E '^#define (CONFIG_GPL|CONFIG_NONFREE|CONFIG_VERSION3) ' config.h > "$prefix_dir/ffmpeg-licence-flags.txt"
    if grep -qE ' 1$' "$prefix_dir/ffmpeg-licence-flags.txt"; then echo "FFmpeg is not LGPLv2.1" >&2; exit 1; fi ;;
libplacebo)
    gitclone https://code.videolan.org/videolan/libplacebo.git v7.387.0 libplacebo
    builddir libplacebo; meson setup .. --cross-file "$prefix_dir/crossfile" -Ddemos=false -Dtests=false \
        -Dopengl=enabled -Dd3d11=enabled -Dvulkan=enabled -Dshaderc=enabled -Dlcms=enabled -Dxxhash=disabled
    install_ ;;
freetype)
    gettar https://download.savannah.gnu.org/releases/freetype/freetype-2.14.3.tar.xz
    builddir freetype-2.14.3; meson setup .. --cross-file "$prefix_dir/crossfile"; install_ ;;
fribidi)
    gettar https://github.com/fribidi/fribidi/releases/download/v1.0.16/fribidi-1.0.16.tar.xz
    builddir fribidi-1.0.16; meson setup .. --cross-file "$prefix_dir/crossfile" -Dtests=false -Ddocs=false; install_ ;;
harfbuzz)
    gettar https://github.com/harfbuzz/harfbuzz/releases/download/14.2.0/harfbuzz-14.2.0.tar.xz
    builddir harfbuzz-14.2.0; meson setup .. --cross-file "$prefix_dir/crossfile" -Dtests=disabled; install_ ;;
libass)
    gitclone https://github.com/libass/libass.git 0.17.5 libass
    builddir libass; meson setup .. --cross-file "$prefix_dir/crossfile"; install_ ;;
mpv)
    # -Dgpl=false: meson refuses every GPL-only feature.
    gitclone https://github.com/mpv-player/mpv.git v0.42.0 mpv
    cd mpv
    CFLAGS="$CFLAGS -I$prefix_dir/include" LDFLAGS="$LDFLAGS -L$prefix_dir/lib" \
    meson setup build --cross-file "$prefix_dir/crossfile" --prefix=/ \
        -Dgpl=false -Dlibmpv=true -Dcplayer=false -Dtests=false \
        -Dlua=disabled -Djavascript=disabled -Dlibcurl=disabled -Dlibarchive=disabled \
        -Dlibbluray=disabled -Ddvdnav=disabled -Duchardet=disabled -Drubberband=disabled \
        -Dzimg=disabled -Dmanpage-build=disabled -Dhtml-build=disabled \
        -Dd3d11=enabled -Dshaderc=enabled -Dspirv-cross=enabled -Dvulkan=enabled \
        -Dgl=enabled -Degl-angle=disabled -Dlcms2=enabled
    ninja -C build
    DESTDIR="$prefix_dir" ninja -C build install
    (grep -E "^ *gpl" build/meson-logs/meson-log.txt | head -5 > "$prefix_dir/mpv-gpl-check.txt" || true)
    # Import library for MSVC (the app is built with MSVC; the C API crosses compilers fine).
    def=$(find . -name 'mpv.def' -not -path './build/*' -print -quit)
    if [ -z "$def" ]; then
        # No .def in this release: list the DLL's exports instead.
        def=/tmp/mpv.def
        $TARGET-objdump -p "$prefix_dir/bin/libmpv-2.dll" > /tmp/mpv.dump
        { echo "EXPORTS"; awk '/\[Ordinal\/Name Pointer\] Table/{f=1; next} f && /^$/{f=0} f && /\]/{print $NF}' /tmp/mpv.dump; } > $def
    fi
    grep -q mpv_create "$def"
    $TARGET-dlltool -d "$def" -D libmpv-2.dll -l "$prefix_dir/lib/mpv.lib" ;;
runtime)
    # MinGW runtime DLLs the bundle needs, then the record of exact sources and licences.
    for f in /usr/lib/gcc/$TARGET/*-posix/libgcc_s_seh-1.dll /usr/lib/gcc/$TARGET/*-posix/libstdc++-6.dll \
             /usr/$TARGET/lib/libwinpthread-1.dll; do cp -v $f "$prefix_dir/bin/"; done
    $TARGET-strip --strip-unneeded "$prefix_dir"/bin/*.dll
    for d in iconv:libiconv-1.19 zlib-ng:zlib-ng-2.3.3 shaderc:shaderc spirv-cross:SPIRV-Cross \
             nv-codec-headers:nv-codec-headers vulkan-loader:Vulkan-Loader dav1d:dav1d lcms2:lcms2 \
             ffmpeg:ffmpeg libplacebo:libplacebo freetype:freetype-2.14.3 fribidi:fribidi-1.0.16 \
             harfbuzz:harfbuzz-14.2.0 libass:libass mpv:mpv; do
        name=${d%%:*}; dir=/src/${d#*:}
        rev=$(git -C "$dir" rev-parse HEAD 2>/dev/null || echo "${d#*:} (release tarball)")
        printf '%-18s %s\n' "$name" "$rev" >> "$prefix_dir/BUILDINFO.txt"
        mkdir -p "$prefix_dir/share/licenses/$name"
        find "$dir" -maxdepth 1 -type f \( -iname 'COPYING*' -o -iname 'LICENSE*' -o -iname 'LICENCE*' \
            -o -iname 'Copyright*' \) -exec cp {} "$prefix_dir/share/licenses/$name/" \;
    done
    for s in shaderc/third_party/glslang shaderc/third_party/spirv-tools; do
        cp "/src/$s/LICENSE"* "$prefix_dir/share/licenses/shaderc/$(basename $s)-LICENSE.txt" 2>/dev/null || true
    done
    echo "SOURCE_DATE_EPOCH ${SOURCE_DATE_EPOCH:-}" >> "$prefix_dir/BUILDINFO.txt" ;;
*) echo "unknown step $1" >&2; exit 2 ;;
esac
