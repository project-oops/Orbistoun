#!/usr/bin/env sh
# Moves the working tree in and out of the generator VM when it cannot be mounted.
#
#   sh tools/toolchain/sync.sh push              # working tree -> VM
#   sh tools/toolchain/sync.sh pull <path>...    # named paths <- VM, into the tree
#
# `setup.sh` mounts the repository, and with a mount there is no step where the two can
# differ - which is why it is the default and this is not. But Multipass ships with
# `local.privileged-mounts` off on Windows, and turning it on is a machine-wide privileged
# setting that belongs to whoever owns the machine, not to a script in a repository. Without
# a mount `run.sh` used to fail inside the guest with `cd: /home/ubuntu/orbistoun: No such
# file or directory`, which reads as a broken VM rather than as a disabled feature.
#
# So: copy in, run, copy back the files the generator writes. **Named** files, one at a
# time, because a whole-tree copy back would carry the guest's `target/` and its `Cargo.lock`
# into the tree and quietly overwrite whatever else had been edited meanwhile.
#
# The generated tables say where they came from, so a table produced this way is not less
# accountable than one produced through a mount - but it is one more step, and a step is
# where a stale copy hides. Pull immediately after the generator runs, never later.
set -e

MSYS_NO_PATHCONV=1
MSYS2_ARG_CONV_EXCL='*'
export MSYS_NO_PATHCONV MSYS2_ARG_CONV_EXCL

NAME=orbistoun-build
MOUNT=/home/ubuntu/orbistoun
REPO=$(cd "$(dirname "$0")/../.." && pwd)
ARCHIVE=orbistoun-tree.tgz

if ! multipass info "$NAME" >/dev/null 2>&1; then
  echo "$NAME does not exist - run tools/toolchain/setup.sh first" >&2
  exit 1
fi

# **Every pulled byte is checked, because two different silent corruptions have already
# happened here.** A copy step that reports success and delivers a different file is
# indistinguishable from a generator that produced one, and the tables it writes are what
# the whole differential suite is judged against.
#
# Its own subcommand as well as part of `pull`, so the guard can be made to fail on demand -
# truncate a file and run `verify` on it. A guard nobody has watched reject something is a
# guard nobody knows anything about.
verify_path() {
  path="$1"
  # Normalised on both sides before comparing, because the two `md5sum`s do not print the
  # same line: Git Bash marks a file read in binary mode with a `*` before its name and
  # separates with one space, GNU coreutils separates with two and marks nothing. Compared
  # raw, every identical file reported as corrupt - a guard that cries wolf is one that gets
  # switched off, which is the failure after the failure.
  # Sorted *after* normalising and under a fixed collation, so the comparison is about
  # content and nothing else. Sorting the file names first left the two sides ordering
  # `operands-primitive` against `operands-probe-...` differently - same files, same hashes,
  # a mismatch reported anyway. A guard that fails for a reason it is not checking teaches
  # people to ignore it.
  guest=$(multipass exec "$NAME" -- sh -lc \
    "cd '$MOUNT' && find '$path' -type f | xargs md5sum" \
    | awk '{ name = $2; sub(/^\*/, "", name); print $1, name }' | LC_ALL=C sort)
  host=$(cd "$REPO" && find "$path" -type f | xargs md5sum \
    | awk '{ name = $2; sub(/^\*/, "", name); print $1, name }' | LC_ALL=C sort)
  if [ "$guest" != "$host" ]; then
    echo "  $path did not survive the copy - the VM and the tree disagree:" >&2
    echo "  in the VM:" >&2
    printf '%s\n' "$guest" | sed 's/^/    /' >&2
    echo "  here:" >&2
    printf '%s\n' "$host" | sed 's/^/    /' >&2
    return 1
  fi
  return 0
}

case "${1:-}" in
  push)
    # Packed outside the repository, because packing it *into* the repository makes `tar`
    # read a directory that is changing under it: it warns and exits non-zero, which under
    # `set -e` ends the push with the archive half written and no explanation.
    #
    # Two spellings of the same path. `tar` here is the shell's, which wants a POSIX path;
    # `multipass` is a native Windows program, which wants a Windows one. `cygpath` does the
    # conversion where it exists and is simply absent on a host where the two coincide.
    work="${TMPDIR:-/tmp}"
    send() {
      from="$1" to="$2"
      archive_path="$work/$ARCHIVE"
      ( cd "$from" && tar --exclude=./target --exclude=./.git --exclude=./titles \
          --exclude=./site -czf "$archive_path" . )
      native="$archive_path"
      if command -v cygpath >/dev/null 2>&1; then
        native=$(cygpath -w "$archive_path")
      fi
      multipass transfer "$native" "$NAME:/home/ubuntu/$ARCHIVE"
      rm -f "$archive_path"
      # Replaced rather than unpacked over: a file deleted here has to be gone there too, or
      # a generator reads a source that no longer exists and its output cannot be reproduced.
      multipass exec "$NAME" -- sh -lc \
        "rm -rf '$to' && mkdir -p '$to' && tar -xzf /home/ubuntu/$ARCHIVE -C '$to'"
    }

    echo "packing the working tree"
    send "$REPO" "$MOUNT"

    # The sibling repositories this workspace builds against, **read out of the manifest**
    # rather than listed here. A path dependency that moves, arrives or goes would otherwise
    # leave this script pushing a set that no longer builds, and the failure surfaces as
    # cargo refusing to read a manifest inside the guest - a long way from the cause.
    siblings=$(grep -oE 'path = "\.\./[A-Za-z0-9_.-]+' "$REPO/Cargo.toml" \
      | sed 's#.*\.\./##' | sort -u)
    for sibling in $siblings; do
      if [ -d "$REPO/../$sibling" ]; then
        echo "packing sibling $sibling"
        send "$REPO/../$sibling" "/home/ubuntu/$sibling"
      else
        echo "  the manifest names ../$sibling and it is not there - the build will fail" >&2
      fi
    done
    echo "the VM now holds this working tree at $MOUNT"
    ;;
  pull)
    shift
    [ $# -gt 0 ] || { echo "pull needs at least one path, relative to the repository root" >&2; exit 2; }
    # `multipass transfer` both ways, never a pipe through `multipass exec`. The pipe
    # version of this hung indefinitely on Windows with no output and no error, which is the
    # worst shape a sync step can fail in: it looks like a slow copy until it is not.
    #
    # **A single file is transferred to its full destination path, never to its directory.**
    # Naming the directory truncated `mnemonics.toml` at exactly 4096 bytes, wrote no error,
    # and exited zero - the table arrived missing a quarter of its rows and looked like a
    # generator that had lost them. Naming the file transfers all of it.
    #
    # And `-` as a destination is not an option either: the shell's redirect adds a carriage
    # return to every line on Windows, so a 5218-byte table arrives as 5566 bytes of the
    # same text. Silent, and worse than the truncation because the file still parses.
    for path in "$@"; do
      parent=$(dirname "$path")
      mkdir -p "$REPO/$parent"
      if multipass exec "$NAME" -- sh -lc "[ -d '$MOUNT/$path' ]"; then
        echo "pulling $path/"
        # Through a staging directory, because `transfer -r` merges into whatever is there -
        # so a file the generator deleted would survive and be read as current - and because
        # deleting first loses the local copy when the transfer then fails. It did: the
        # recursive transfer exits non-zero on this host, and the directory was already gone.
        staging="$REPO/$parent/.sync-staging"
        rm -rf "$staging"
        mkdir -p "$staging"
        destination="$staging"
        if command -v cygpath >/dev/null 2>&1; then
          destination=$(cygpath -w "$destination")
        fi
        # **The exit status is not the oracle here; the checksum below is.** `multipass
        # transfer` reports failure on Windows for every file it cannot chmod on an NTFS
        # volume, having copied the bytes correctly - so believing it would refuse every
        # good pull, and believing the opposite would accept a bad one. The bytes decide.
        transferred=0
        multipass transfer -r "$NAME:$MOUNT/$path" "$destination" || transferred=$?
        base=$(basename "$path")
        if [ ! -d "$staging/$base" ]; then
          echo "  nothing arrived for $path (transfer exit $transferred)" >&2
          rm -rf "$staging"
          exit 1
        fi
        rm -rf "$REPO/$path"
        mv "$staging/$base" "$REPO/$path"
        rm -rf "$staging"
      else
        echo "pulling $path"
        destination="$REPO/$path"
        if command -v cygpath >/dev/null 2>&1; then
          destination=$(cygpath -w "$destination")
        fi
        multipass transfer "$NAME:$MOUNT/$path" "$destination" || true
      fi

      verify_path "$path" || exit 1
    done
    ;;
  verify)
    shift
    [ $# -gt 0 ] || { echo "verify needs at least one path, relative to the repository root" >&2; exit 2; }
    failed=0
    for path in "$@"; do
      if verify_path "$path"; then
        echo "  $path matches the VM"
      else
        failed=1
      fi
    done
    exit $failed
    ;;
  *)
    echo "usage: sync.sh push | sync.sh pull <path>... | sync.sh verify <path>..." >&2
    exit 2
    ;;
esac
