class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.6.5"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.5/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "49d05c454d2417bbe39171f50d06c06081c42fac2596c3ce43a7644c5621ccc3"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.5/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "3e9b8f5bb918772a96b5e21458dbbfebeee3daa3399db1f372c5bc92264d3e6b"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.5/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "0dc63bbaf8b74fa9b85d8b59e2db8d4888b52e792e47da7d41d7eaa4e0722e8d"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.5/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "dbf320aa8a3d1b8068d907c49fe5284ca13f0e99bb7a514e9d563fa589a4a938"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
