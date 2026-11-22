class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.12.4"
  # Custom noncommercial license (LightSpeed-NC-1.1), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.4/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "a58a03e7e55885135111ac781b1be0c9e406a0a1a44c37b8800c2bb3a0ab1d26"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.4/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "a3770412af21c41320b081f95706716cc378657bd237a9e6752710a2f9f2a226"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.4/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "3913b3a28a66077e8ba69e1a8c00d458955f7b5093eaf270c23ffefb91d1a316"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.4/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "efe4aa508eb3b1041f9c03a61daa98d0c8fb34e0c233526458948917c43bdd1f"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
