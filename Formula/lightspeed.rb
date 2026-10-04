class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.7.0"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.7.0/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "647bb4f50a28ec73f74152a8bac4e5a7fb86a44d1a3ef5165cbd5d6ee6a08f7e"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.7.0/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "6d21d211bc0c27d10b0a1a4258a7a582a8a9b7072a5fb2650bbf8330127aecc6"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.7.0/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "785a2b8ee5110af3cee0d5b0fdf5ec5a8aa22e850577f0603544ff29ab11dabf"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.7.0/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "b506d67ec3b8221b0aff4e42d53d217d247842ee3046c2133e4a1d348bbea14c"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
