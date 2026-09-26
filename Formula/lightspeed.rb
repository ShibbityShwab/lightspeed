class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.6.14"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.14/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "bc3ce7d4e207d76fa926458058b1248719fa29c7b985d4e26b7bc025ae752b91"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.14/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "db93b719c78924013daf829d8f9d2cee6f50020863e5e8db83c653d047464c43"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.14/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "0f6b094517b2931ab3609dddfeead68f557abd4097c5a4bb303103548eb56c65"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.14/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "ce208dbcd81ee3b624eb8cdd319b8995248f21782d1eca116c45edd76b70cefd"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
