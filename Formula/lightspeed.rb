class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.6.2"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.2/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "b8b0aafb4b7eaa631c065e1e924e8fee523b0ff23b49ebd2a4118b64693da886"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.2/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "e7d3f0f778dff1f56bae6fe9725ebe5f088d00b72ef493a325245e495db39a95"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.2/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "044e53f1d4b641755fa4b5c4d2a48417006027c1a0616abaad85c0fa7b2ce67c"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.2/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "6aae73898e7cb5c295839043f5469cdec2a657fb50d39c94ae84b49a878bed86"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
