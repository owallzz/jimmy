# Binary package: build-rpm.sh compiles jimmy with cargo (or takes a prebuilt JIMMY_BIN), then
# this spec packages it.
%global debug_package %{nil}

Name:           jimmy
Version:        %{jimmy_version}
Release:        %{jimmy_release}%{?dist}
Summary:        Lightweight, gamepad-friendly media player
%if 0%{?jimmy_system_mpv}
License:        MIT
%else
# Static builds include mpv (GPL-2.0-or-later as built), FFmpeg and libplacebo (LGPL), dav1d
# (BSD-2-Clause), libass (ISC) and libdisplay-info (MIT).
License:        MIT AND GPL-2.0-or-later AND LGPL-2.1-or-later AND BSD-2-Clause AND ISC
%endif
URL:            https://github.com/owallzz/jimmy

%if 0%{?jimmy_system_mpv}
# Built against the system libmpv. Install RPM Fusion's ffmpeg for H.264/HEVC (see README).
Requires:       mpv-libs
%endif
# Otherwise mpv, FFmpeg and libass are built into jimmy (packaging/build-slim-mpv.sh); the small
# system libraries they use (libva, libpulse, freetype, fontconfig…) are picked up automatically.
# System volume (wpctl) and backlight/screen-awake helpers.
Recommends:     wireplumber
# Posters, metadata and subtitle downloads.
Requires:       curl
Recommends:     systemd

%description
jimmy is a fast, utilitarian media player for old hardware: a media browser built
for slow disks, full gamepad and keyboard (vim-style) control, command palette,
remappable controls, subtitles and closed captions, system volume with boost up
to 200%%, backlight control and 20 color schemes. Playback is done by libmpv.

%prep

%build

%install
install -Dm755 %{jimmy_bin} %{buildroot}%{_bindir}/jimmy
install -Dm644 %{jimmy_src}/assets/jimmy.desktop %{buildroot}%{_datadir}/applications/jimmy.desktop
install -Dm644 %{jimmy_src}/assets/jimmy.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/jimmy.svg
install -Dm644 %{jimmy_src}/README.md %{buildroot}%{_docdir}/jimmy/README.md
install -Dm644 %{jimmy_src}/LICENSE %{buildroot}%{_licensedir}/jimmy/LICENSE
install -Dm644 %{jimmy_src}/THIRD-PARTY.md %{buildroot}%{_licensedir}/jimmy/THIRD-PARTY.md

%files
%{_bindir}/jimmy
%{_datadir}/applications/jimmy.desktop
%{_datadir}/icons/hicolor/scalable/apps/jimmy.svg
%doc %{_docdir}/jimmy/README.md
%license %{_licensedir}/jimmy/LICENSE
%license %{_licensedir}/jimmy/THIRD-PARTY.md

%changelog
* Sat Nov 07 2026 jimmy developers - %{jimmy_version}-%{jimmy_release}
- Release build
