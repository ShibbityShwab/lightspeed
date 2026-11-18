class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.11.3"
  # Custom noncommercial license (LightSpeed-NC-1.1), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.3/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "62818ad2d134283dae3b08a94914a1794e64c30f167bcd844c4e3f1a86876353"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.3/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "034d3698b39bec1019d6c4fcb2452b3831f3001235d4a502d8c2cec158af3c2f"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.3/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "00108ff777be2aa0ebd4f05b0cb943943b499a5ab1c720b0ec70959493be84b6"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.3/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "0de78fe31139f4bb05c8ee63c5a5332b9d3999d6667212e26ee22dfa9539d945"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
