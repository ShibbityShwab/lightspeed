class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.12.2"
  # Custom noncommercial license (LightSpeed-NC-1.1), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.2/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "ac2fde303833e76bb63356df916cafa7dd8829fb5c99c8e3aae5f65da2ebfbe7"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.2/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "3c03575298847b98fe824771cb2ee984279da46b48dfda1e98a4f221057aa9d5"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.2/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "e8fbc05269df6b5f139dab063884376c75f614a35da3d038dd4d4436315868e7"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.2/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "a49dfa260437258ce72e95fd0ee4d84805a6fee0edfc6c7ad70c151b2cfd5b26"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
