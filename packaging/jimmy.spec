# Binary package: build-rpm.sh compiles jimmy with cargo first, then this spec packages it.
%global debug_package %{nil}

Name:           jimmy
Version:        %{jimmy_version}
Release:        1%{?dist}
Summary:        Lightweight, gamepad-friendly media player
License:        LicenseRef-Proprietary
URL:            https://github.com/owallzz/jimmy

# libmpv. Install RPM Fusion's ffmpeg for H.264/HEVC support (see README).
Requires:       mpv-libs
# System volume (wpctl) and backlight/screen-awake helpers.
Recommends:     wireplumber
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

%files
%{_bindir}/jimmy
%{_datadir}/applications/jimmy.desktop
%{_datadir}/icons/hicolor/scalable/apps/jimmy.svg
%doc %{_docdir}/jimmy/README.md

%changelog
* Sat Nov 07 2026 jimmy developers - %{jimmy_version}-1
- Release build
