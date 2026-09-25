class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.6.11"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.11/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "6b6c4d19143d82c0cde7bb0806fdec77b2b85d23fe88720438b283f4d79d0d5d"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.11/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "297db946e2dfa293e613373eef171d968aec7ff449e1a33670802fe09e62aed6"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.11/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "c31a98dd77eafd08665ee816226fa9b7f92c5629740b258ce981329c8f958e1b"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.11/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "adb3a3bf05e50fb9165b53c2a58016b84ef81273b4534f46552eabe61fa3f9cc"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
