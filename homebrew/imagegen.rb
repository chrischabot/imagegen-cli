# Homebrew Formula template for imagegen.
#
# On each release, release.sh copies this file to the tap repo
# (github.com/chrischabot/homebrew-imagegen, under Formula/imagegen.rb),
# filling in `version` and `sha256` from the release tarball.
#
# Users then install with:
#     brew tap chrischabot/imagegen
#     brew install imagegen
class Imagegen < Formula
  desc "Fast, agent-friendly CLI for OpenAI image generation (gpt-image-2)"
  homepage "https://github.com/chrischabot/imagegen-cli"
  url "https://github.com/chrischabot/imagegen-cli/archive/refs/tags/v0.0.0.tar.gz"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"
  license "MIT"
  head "https://github.com/chrischabot/imagegen-cli.git", branch: "main"

  livecheck do
    url :stable
    strategy :github_latest
  end

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/imagegen --version")
    # No API key in the test environment: expect the auth error path (exit 2).
    output = shell_output("OPENAI_API_KEY= #{bin}/imagegen generate hello 2>&1", 2)
    assert_match "no API key found", output
  end
end
