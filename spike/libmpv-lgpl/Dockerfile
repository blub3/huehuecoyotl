# M0 spike: reproducible LGPL build of libmpv + FFmpeg (+ libass, libplacebo).
# Base matches the oldest Tier-1 Linux target we care about (Ubuntu 24.04 glibc).
# Output: /opt/coyotl-mpv (shared libraries only, rpath $ORIGIN) + BUILDINFO.txt
FROM ubuntu:24.04

ARG FFMPEG_TAG=n8.1.3
ARG NVCODEC_TAG=n13.0.19.0
ARG LIBASS_TAG=0.17.5
ARG LIBPLACEBO_TAG=v7.387.0
ARG MPV_TAG=v0.42.0
ENV SOURCE_DATE_EPOCH=1762732800 \
    DEBIAN_FRONTEND=noninteractive \
    PREFIX=/opt/coyotl-mpv

# Build tools and system libraries that every desktop already ships at runtime.
# None of these are GPL; the CI licence check is in check-licence.sh.
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates git build-essential pkg-config nasm meson ninja-build \
      autoconf automake libtool python3-jinja2 python3-markupsafe \
      libfreetype-dev libfribidi-dev libharfbuzz-dev libfontconfig-dev \
      libva-dev libdrm-dev libegl-dev libgl-dev libgles-dev \
      libwayland-dev wayland-protocols libxkbcommon-dev \
      libpulse-dev libpipewire-0.3-dev libasound2-dev liblcms2-dev zlib1g-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
ENV PKG_CONFIG_PATH=/opt/coyotl-mpv/lib/pkgconfig \
    LD_LIBRARY_PATH=/opt/coyotl-mpv/lib

# NVIDIA codec headers (MIT): enable nvdec/cuvid without linking anything NVIDIA at build time.
RUN git clone --depth 1 -b ${NVCODEC_TAG} https://github.com/FFmpeg/nv-codec-headers.git \
    && make -C nv-codec-headers PREFIX=${PREFIX} install

# FFmpeg: LGPL v2.1+. No --enable-gpl, no --enable-nonfree, no --enable-version3.
# Decoders/demuxers/parsers/filters only; no encoders, muxers or devices.
RUN git clone --depth 1 -b ${FFMPEG_TAG} https://github.com/FFmpeg/FFmpeg.git ffmpeg \
    && cd ffmpeg && ./configure --prefix=${PREFIX} \
       --enable-shared --disable-static --disable-programs --disable-doc \
       --disable-encoders --disable-muxers --disable-indevs --disable-outdevs \
       --disable-autodetect \
       --enable-zlib --enable-vaapi --enable-ffnvcodec --enable-nvdec --enable-cuvid \
       --enable-network --enable-pic \
       --extra-ldflags='-Wl,-rpath,$$ORIGIN' \
    && make -j"$(nproc)" && make install \
    && grep -E '^#define (CONFIG_GPL|CONFIG_NONFREE|CONFIG_VERSION3) ' config.h > ${PREFIX}/ffmpeg-licence-flags.txt \
    && grep FFMPEG_VERSION libavutil/ffversion.h >> ${PREFIX}/ffmpeg-licence-flags.txt

# libass: ISC
RUN git clone --depth 1 -b ${LIBASS_TAG} https://github.com/libass/libass.git \
    && cd libass && ./autogen.sh && ./configure --prefix=${PREFIX} --disable-static \
    && make -j"$(nproc)" && make install

# libplacebo: LGPL v2.1+. OpenGL only (embedded mode); Vulkan left out for the spike.
RUN git clone --depth 1 --recurse-submodules -b ${LIBPLACEBO_TAG} https://code.videolan.org/videolan/libplacebo.git \
    && cd libplacebo && meson setup build --prefix=${PREFIX} --libdir=lib --buildtype=release \
       -Dvulkan=disabled -Dopengl=enabled -Dshaderc=disabled -Dglslang=disabled \
       -Dd3d11=disabled -Ddemos=false -Dtests=false -Dlcms=enabled -Dxxhash=disabled \
    && ninja -C build install

# libwayland (MIT) + wayland-protocols: mpv 0.42 needs wayland >= 1.23, Ubuntu 24.04 has 1.22.
# Bundled; it is backward compatible with the system copy that Qt and the GL driver use.
ARG WAYLAND_TAG=1.24.0
ARG WAYLAND_PROTOCOLS_TAG=1.45
RUN apt-get update && apt-get install -y --no-install-recommends libffi-dev libexpat1-dev libxml2-dev \
    && rm -rf /var/lib/apt/lists/* \
    && git clone --depth 1 -b ${WAYLAND_TAG} https://gitlab.freedesktop.org/wayland/wayland.git \
    && cd wayland && meson setup build --prefix=${PREFIX} --libdir=lib --buildtype=release \
       -Ddocumentation=false -Dtests=false \
    && ninja -C build install && cd .. \
    && git clone --depth 1 -b ${WAYLAND_PROTOCOLS_TAG} https://gitlab.freedesktop.org/wayland/wayland-protocols.git \
    && cd wayland-protocols && PATH=${PREFIX}/bin:$PATH meson setup build --prefix=${PREFIX} -Dtests=false \
    && ninja -C build install
ENV PKG_CONFIG_PATH=/opt/coyotl-mpv/lib/pkgconfig:/opt/coyotl-mpv/share/pkgconfig \
    PATH=/opt/coyotl-mpv/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

# mpv: -Dgpl=false. meson refuses every GPL-only feature (x11, dvdnav, cdda, jack, ...).
RUN git clone --depth 1 -b ${MPV_TAG} https://github.com/mpv-player/mpv.git \
    && cd mpv && meson setup build --prefix=${PREFIX} --libdir=lib --buildtype=release \
       -Dgpl=false -Dlibmpv=true -Dcplayer=false -Dtests=false \
       -Dlua=disabled -Djavascript=disabled -Dvulkan=disabled \
       -Dx11=disabled -Dwayland=enabled -Degl=enabled -Dgl=enabled \
       -Dvaapi=enabled -Dpulse=enabled -Dpipewire=enabled -Dalsa=enabled \
       -Dlibarchive=disabled -Dlibbluray=disabled -Duchardet=disabled -Drubberband=disabled \
       -Dzimg=disabled -Dlcms2=enabled -Dmanpage-build=disabled -Dhtml-build=disabled \
       -Dc_link_args='-Wl,-rpath,$ORIGIN' \
    && ninja -C build install \
    && (grep -E "^ *gpl" build/meson-logs/meson-log.txt | head -5 > ${PREFIX}/mpv-gpl-check.txt || true)

# Record exact source revisions for the "sources under our control" obligation.
RUN for d in nv-codec-headers ffmpeg libass libplacebo wayland wayland-protocols mpv; do \
      printf '%-18s %s\n' "$d" "$(git -C $d rev-parse HEAD)"; done > ${PREFIX}/BUILDINFO.txt \
    && echo "SOURCE_DATE_EPOCH ${SOURCE_DATE_EPOCH}" >> ${PREFIX}/BUILDINFO.txt
