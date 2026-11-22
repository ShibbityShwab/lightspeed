class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.12.3"
  # Custom noncommercial license (LightSpeed-NC-1.1), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.3/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "687e915346fc7903337fb194e37d4dac0df778d42d2fbf360b2830a8a5814353"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.3/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "c032eb723fd0fc871391a73965e0cefb708150cde3e10bff97a202d26ee8b378"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.3/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "673327db91f4a02b54e88f4d81fd5ab575082fd0fb15b181f8c11755f10bf21d"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.3/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "06d9f4698f0b7c5f890d3bc2f7f2f25c7177bb60cb5727cc2a20ced86fbb1eba"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
