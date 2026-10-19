class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.8.1"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.8.1/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "ac8ac05087ebceb1f93e7d2365030b1857c46875f64ba02b7826f8131430a01e"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.8.1/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "c681ea88a108d6cda185831bc96a2e449dda9240ab43289f64be98f387b854e2"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.8.1/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "edc7e8b088dcbf0a02e69678d1fb5a52614f4a6508f868ce97612fe8888484f8"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.8.1/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "23bffe77e1ecfd79a557cb99959c686b9602233d75429036aaa79cad3862bfcc"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
