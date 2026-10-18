class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.8.0"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.8.0/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "2fe5a0684fccd3d0d1762432310cb9819045285bf70fd2b5045a282d04a71bef"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.8.0/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "701cab289e10d6ee554ca97334bc40518f0930fea9a09b6e2f2c76936a5d70e5"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.8.0/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "ec3fb402796432db51b14b9be9f3f0a752c27d0599c19c60977b2b470018fcc1"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.8.0/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "0cb6d0339575fb3ea4d29c00facc67428b2b05e9d8528d27b53b9f640bc6d314"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
