#!/usr/bin/env python3
"""Run the actual Java directory-repair helper; no Android dependency fixtures."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
fixture = r'''
package com.termux.app;
import java.nio.file.*;
import java.util.*;
import java.io.*;
public class DirectoryRepairProbe {
 static void check(boolean value) { if (!value) throw new AssertionError(); }
 static void reject(Path prefix) throws Exception {
  try { BootstrapDirectoryRepair.ensure(prefix.toFile()); throw new AssertionError("expected refusal"); }
  catch (IOException expected) { }
 }
 public static void main(String[] args) throws Exception {
  Path root = Paths.get(args[0]);
  Path good=Files.createDirectory(root.resolve("good"));
  byte[] payload="user-owned-data".getBytes(java.nio.charset.StandardCharsets.UTF_8);
  Files.write(good.resolve("keep.txt"),payload);
  BootstrapDirectoryRepair.ensure(good.toFile());
  check(Files.isDirectory(good.resolve("tmp")) && Files.isDirectory(good.resolve("etc/apt/apt.conf.d")));
  BootstrapDirectoryRepair.ensure(good.toFile());
  check(Arrays.equals(payload,Files.readAllBytes(good.resolve("keep.txt"))));
  Path conflict=Files.createDirectory(root.resolve("conflict"));
  Files.write(conflict.resolve("tmp"),payload); reject(conflict);
  check(Arrays.equals(payload,Files.readAllBytes(conflict.resolve("tmp"))));
  check(!Files.exists(conflict.resolve("etc")));
  Path outside=Files.createDirectory(root.resolve("outside"));
  Path escape=Files.createDirectory(root.resolve("escape"));
  Files.createSymbolicLink(escape.resolve("etc"),outside); reject(escape);
  check(!Files.exists(escape.resolve("tmp")) && !Files.exists(outside.resolve("apt")));
  check(Files.isSymbolicLink(escape.resolve("etc")));
  Path supplied=Files.createDirectory(root.resolve("supplied"));
  Files.createSymbolicLink(supplied.resolve("tmp"),outside);
  BootstrapDirectoryRepair.ensure(supplied.toFile());
  check(Files.isSymbolicLink(supplied.resolve("tmp")));
  try (java.util.stream.Stream<Path> entries=Files.list(outside)) { check(entries.findAny().isEmpty()); }
  Path notDirectory=root.resolve("file-prefix"); Files.write(notDirectory,payload); reject(notDirectory);
  System.out.println("PASS actual Java helper: missing directories, idempotence, user bytes retained, conflict refusal, symlink escape refusal, existing directory symlink retained");
 }
}
'''
with tempfile.TemporaryDirectory(prefix='bootstrap-repair-') as directory:
    root = Path(directory)
    test = root / 'DirectoryRepairProbe.java'; test.write_text(fixture)
    helper = ROOT / 'app/src/main/java/com/termux/app/BootstrapDirectoryRepair.java'
    subprocess.run(['javac', '-d', directory, str(helper), str(test)], check=True)
    subprocess.run(['java', '-cp', directory, 'com.termux.app.DirectoryRepairProbe', directory], check=True)
source = (ROOT / 'app/src/main/java/com/termux/app/TermuxInstaller.java').read_text()
section = source[source.index('if (prefixExists)'):source.index('// Step 5: Start bootstrap installation')]
assert 'BootstrapDirectoryRepair.ensure' in section
assert 'Never fall through to the destructive fresh-install path' in section
assert 'deleteFile(' not in section
print('PASS existing-prefix wiring; full Activity execution remains an ART boundary')
