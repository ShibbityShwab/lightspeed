class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.5.0"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.5.0/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "1a049459de44397280617d8bd063559377c9065274ce67dfbdeb1a9d27be2385"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.5.0/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "74df67217d8c90e21785d97deeacb7c96a148adde6c660205adc54746d68def6"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.5.0/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "994a8a6eb534f6bc6b4d9dc6ea9b7972173fd68641e916f8146f850f023dc284"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.5.0/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "b093a194a13a1110c49947d2309bddb796c8677dfa05e7830a2afb9b828535c8"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
