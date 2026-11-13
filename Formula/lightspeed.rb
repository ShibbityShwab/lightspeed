class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.10.1"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.10.1/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "1368e3d144b4072fd96f7b875f4565ab3bcd1d21bb7616f540e71f5f436c60bc"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.10.1/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "f36ecd73046958a0eedd8116e84f238ddcdd97d8cc2eeaae8a074cd5fde3730c"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.10.1/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "c9525c980691dedcdb8325d89045c8639bd41b8214d375ccc707d8dd23b6598c"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.10.1/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "1509320633d95fe35a2674150bca2cf3cd7c6d3c6b290cccc6b8ab29712229fd"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
