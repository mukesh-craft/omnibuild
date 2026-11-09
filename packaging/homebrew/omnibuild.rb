class Omnibuild < Formula
  desc "Local-first build orchestration and artifact verification"
  homepage "https://github.com/mukesh-craft/omnibuild"
  version "0.1.1"
  license :cannot_represent

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/mukesh-craft/omnibuild/releases/download/v0.1.1/omnibuild-0.1.1-aarch64-apple-darwin.tar.gz"
      sha256 "a12392676c97a9b648b77a27d30789b85d3155a3acf2b374903051928acf0b70"
    end
    if Hardware::CPU.intel?
      url "https://github.com/mukesh-craft/omnibuild/releases/download/v0.1.1/omnibuild-0.1.1-x86_64-apple-darwin.tar.gz"
      sha256 "REPLACE_WITH_X86_64_APPLE_DARWIN_SHA256"
    end
  end

  depends_on :macos

  livecheck do
    url :stable
    strategy :github_latest
  end

  def install
    bin.install "omnibuild"
    doc.install "README.md", "LICENSE", "COPYING"
  end

  test do
    assert_match "omnibuild #{version}", shell_output("#{bin}/omnibuild --version")
    system "#{bin}/omnibuild", "targets"
  end
end
