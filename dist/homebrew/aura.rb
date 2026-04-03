# Homebrew formula for AURA — AI-native terminal text editor
# To install: brew install odtorres/aura/aura
#
# This formula builds from source. For pre-built binaries, use:
#   curl --proto '=https' --tlsv1.2 -LsSf https://github.com/odtorres/aura/releases/latest/download/aura-installer.sh | sh

class Aura < Formula
  desc "AI-native terminal text editor for human + AI co-authoring"
  homepage "https://github.com/odtorres/aura"
  url "https://github.com/odtorres/aura/archive/refs/tags/v0.2.2.tar.gz"
  sha256 "PLACEHOLDER"
  license "MIT"
  head "https://github.com/odtorres/aura.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "build", "--release", "--package", "aura"
    bin.install "target/release/aura"
  end

  test do
    assert_match "AURA", shell_output("#{bin}/aura --version")
  end
end
