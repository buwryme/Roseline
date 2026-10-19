PREFIX ?= /usr/local
VERSION := 0.1.0
CARGO ?= cargo
STAGING := $(CURDIR)/build/stage

all: build

build:
	$(CARGO) build --release

install:
	install -Dm755 target/release/roseline "$(DESTDIR)$(PREFIX)/bin/roseline"
	install -Dm644 data/net.buwryy.Roseline.desktop "$(DESTDIR)$(PREFIX)/share/applications/net.buwryy.Roseline.desktop"
	install -Dm644 data/net.buwryy.Roseline.Settings.desktop "$(DESTDIR)$(PREFIX)/share/applications/net.buwryy.Roseline.Settings.desktop"
	install -Dm644 data/net.buwryy.Roseline.metainfo.xml "$(DESTDIR)$(PREFIX)/share/metainfo/net.buwryy.Roseline.metainfo.xml"
	install -Dm644 data/icons/hicolor/scalable/apps/net.buwryy.Roseline.svg "$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/net.buwryy.Roseline.svg"
	install -Dm644 data/icons/hicolor/128x128/apps/net.buwryy.Roseline.png "$(DESTDIR)$(PREFIX)/share/icons/hicolor/128x128/apps/net.buwryy.Roseline.png"
	install -Dm644 LICENSE "$(DESTDIR)$(PREFIX)/share/licenses/roseline/LICENSE"
	install -Dm644 NOTICE "$(DESTDIR)$(PREFIX)/share/licenses/roseline/NOTICE"
	-@[ -n "$(DESTDIR)" ] || update-desktop-database "$(PREFIX)/share/applications" 2>/dev/null || true
	-@[ -n "$(DESTDIR)" ] || xdg-mime default net.buwryy.Roseline.desktop x-scheme-handler/roblox 2>/dev/null || true
	-@[ -n "$(DESTDIR)" ] || xdg-mime default net.buwryy.Roseline.desktop x-scheme-handler/roblox-player 2>/dev/null || true

uninstall:
	rm -f "$(DESTDIR)$(PREFIX)/bin/roseline" \
	      "$(DESTDIR)$(PREFIX)/share/applications/net.buwryy.Roseline.desktop" \
	      "$(DESTDIR)$(PREFIX)/share/applications/net.buwryy.Roseline.Settings.desktop" \
	      "$(DESTDIR)$(PREFIX)/share/metainfo/net.buwryy.Roseline.metainfo.xml" \
	      "$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/net.buwryy.Roseline.svg" \
	      "$(DESTDIR)$(PREFIX)/share/icons/hicolor/128x128/apps/net.buwryy.Roseline.png" \
	      "$(DESTDIR)$(PREFIX)/share/licenses/roseline/LICENSE" \
	      "$(DESTDIR)$(PREFIX)/share/licenses/roseline/NOTICE"
	-@if [ -z "$(DESTDIR)" ]; then \
	    if command -v gtk-update-icon-cache >/dev/null 2>&1; then \
	        gtk-update-icon-cache -qtf "$(PREFIX)/share/icons/hicolor" 2>/dev/null || \
	            rm -f "$(PREFIX)/share/icons/hicolor/icon-theme.cache"; \
	    else \
	        rm -f "$(PREFIX)/share/icons/hicolor/icon-theme.cache"; \
	    fi; \
	fi

# a source tarball, the top directory named as rpmbuild expects
dist:
	mkdir -p build
	tar --exclude=./target --exclude=./build --exclude-vcs \
	    --transform 's,^\.,roseline-$(VERSION),' \
	    -czf build/roseline-$(VERSION).tar.gz .

# a plain binary tarball, laid out as a system prefix
dist-bin:
	rm -rf "$(STAGING)"
	$(MAKE) install DESTDIR="$(STAGING)" PREFIX=/usr
	strip --strip-unneeded "$(STAGING)/usr/bin/roseline" || true
	tar -C "$(STAGING)" -cJf build/roseline-$(VERSION)-x86_64.tar.xz .
	rm -rf "$(STAGING)"

# an arch package; build-aux/PKGBUILD is the native route
dist-arch:
	./build-aux/make-arch.sh

clean:
	$(CARGO) clean
	rm -rf build

.PHONY: all build install uninstall dist dist-bin dist-arch clean
