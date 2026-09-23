class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.6.9"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.9/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "3e78eb63721b6d28068cc93b400ab674c539ff61b4d1d4de3657745aee9e4e01"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.9/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "a6af5962f6d3af662fa54324c8398e45f338882650cfd63bbd382d0ef212b301"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.9/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "3ba62bdbf908a498742f55f83e6c3ef19c90491219108bf1b3371f63f9d27e6e"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.9/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "8200173641d48b177b99b65b703139735033a400bb0de616e6233d516988f044"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
