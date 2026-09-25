class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.6.13"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.13/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "094dbcac35555dac5fffd3fd1a1acdb2a62da1664aa6648bee8b8dabd33b8db8"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.13/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "3f68cad52d60783d08b2050a513f942df024366c217d814b4aa0abf8042c24ff"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.13/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "f964e0dba4bb6f8ebd0f45aec7254efc344b75e05a0b32a369a08c2fd641bb56"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.13/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "0c8b0076ffd6c42c81429969cf1c737d74f46bd379ae4a0b9f1c1c575eb70a38"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
