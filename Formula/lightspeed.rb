class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.12.1"
  # Custom noncommercial license (LightSpeed-NC-1.1), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.1/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "019136e7eeb300f332827b7eb12e8eadd0e3dbcba44f5f14cc19e53c3d6e795b"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.1/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "cdeac05de8287022542048f461a7ac487d6244393877a82f6e281dd52528141c"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.1/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "3c3f4431e6000dca41ad82e546b5205735846d9013354ea58077d1536a323eb9"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.1/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "f6407528588bbe0b36d9f0eef632eadda0be15ab3f426f14aea73e3f59fe11ce"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
