class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.8.3"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.8.3/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "f82ebbda36a7429fedef4a0933f80370d160c67ec4085c460d650907055eb49b"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.8.3/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "c672bbf1dc2035b1a070bc9ee630cc63c4d80418ebd4635f2f2794e5f6ebac08"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.8.3/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "18213222a4020c20b616967c02005b4b6ecf23b843c073f5b1dfe26e8890a47d"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.8.3/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "155e2d2f7555f6e2174b29e1b1e8276fef86c5b69ef2a15900dbb25f7264ff9b"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
