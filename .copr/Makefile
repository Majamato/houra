# Builds Houra's source RPM for COPR's "make_srpm" SCM method. COPR runs
#   make -f .copr/Makefile srpm outdir=<dir> spec=<spec>
# as root in a disposable chroot, from a checkout of the tag being built.

top_dir := $(abspath $(dir $(lastword $(MAKEFILE_LIST)))..)
outdir ?= $(top_dir)/build-srpm

.PHONY: srpm
srpm:
	dnf -y install cargo git-core rpm-build tar xz
	git config --global --add safe.directory $(top_dir)
	$(top_dir)/scripts/make-release-tarballs.sh $(outdir)
	rpmbuild -bs \
		--define '_sourcedir $(outdir)' \
		--define '_srcrpmdir $(outdir)' \
		$(top_dir)/packaging/fedora/houra.spec
