class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.6.7"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.7/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "2225c31ffca0911bff1d63cc1dbad15508e4ce736dfd76f8c06bcb479a2c1112"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.7/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "fe50805b246c7ad89ecb6b2407ab5daa03382966317ba4e0b88f052c186e5d79"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.7/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "b2a1b31b4782c48f23e2ba7344cd708d9a88ad32106f871deb7ef169fe3d129a"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.7/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "72dc0f8ac1d615f8b8fb41ae261ee2ce3065fecae6f9f0d3e223726700583884"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
