class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.6.0"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.0/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "301d7761dd36f2a7f9a30341add21d6efb5a851d5e7c033a6d2e9c64fde69fdc"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.0/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "b3a2038b48e57e917ba0e8a65b306d30cbc7a164a9e9587f30e10d951c5066c3"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.0/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "7ab23d980b6b8ba5682a475872ce9bf25c2b35f0e40e651522e0a03346141de7"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.0/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "fc0574b6a136415592b75c3289fa0f9b5e9a76ef256279a8a28a6f5462de1c31"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
