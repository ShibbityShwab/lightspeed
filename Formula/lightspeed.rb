class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.11.1"
  # Custom noncommercial license (LightSpeed-NC-1.1), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.1/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "17cb0470feb6e13ac359721989f41f2311cac7ba899aca0fcbd35ae57897f814"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.1/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "36dbdd69db9ac7ebd130983ded5076055f233d9801d0f51cccc192ae0007cea7"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.1/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "ae0fd50a33df177f48dc098af017b75107db165ff1b1ef23289d333b011299ad"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.1/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "36a823484a243d8720c2419bed7f38a66d595821f69fc21edb808b35444be08b"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
