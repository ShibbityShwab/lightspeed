class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.8.2"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.8.2/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "9983ef55f0e9d6d76015720a3035f5217b54782c34eef58772348ad9c790310d"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.8.2/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "fe9637d9638f578dbbd0bda15d865ce8f0796d3647720d59e61c75b89975aba7"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.8.2/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "c385620f21edd888f75f91037e841ddf4887fd85d4441253418203d28177adb2"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.8.2/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "624df0b56a9f7f0a13d0ff5da25359cfc02da697252b8adf72898dfb410167c7"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
