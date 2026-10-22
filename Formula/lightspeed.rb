class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.8.4"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.8.4/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "392ee02870a73927a5424f47bfe4bd8f33d9fceab87eef72c1264be348cea38a"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.8.4/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "063db9f93709699fadbb759fd31dfbf483b6229bf32d157543a867b441df82a6"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.8.4/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "2e2106f2d704d807e1baa474d23611d8af1ff117bc90dc3bab6042d6574a2061"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.8.4/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "d4409891cefcb2f46e275b0f9117b9e98e7d324f007e802f452ea5cdfd50e76a"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
