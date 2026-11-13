class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.10.0"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.10.0/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "bae1480804a8017dbfe68e6fdbd0a0015c97cc5ed3ba1640b961d34b69322cd9"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.10.0/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "fea7988f39f82db87642ea6475baabb9598a40c3849b170d4234928687198f8f"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.10.0/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "b0d9e297facb62b3ce89739a0659c4328f01b57305af8c503beaad23bb6629b9"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.10.0/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "cf603363f6a6e60c3bf46d8040706b2c122789dfd1b7bf4939eb82ff8a521ae4"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
